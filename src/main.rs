use aws_config::meta::region::RegionProviderChain;
use aws_sdk_dynamodb::model::AttributeValue;
use aws_sdk_dynamodb::Client;
use serde::{Deserialize, Serialize};
use validator::Validate;
use warp::reply::Json;
use warp::{http::StatusCode, Filter, Rejection, Reply};

use std::collections::HashMap;
use std::convert::Infallible;

use std::fmt;

use thiserror::Error;

use std::time::{SystemTime, UNIX_EPOCH};

use std::str::FromStr;
use strum_macros::EnumString;

mod words;

const DEFAULT_AWS_REGION: &str = "ap-south-1";
const TABLE_NAME: &str = "rusty_wordlet_games";
const MAX_GUESSES: usize = 5;
const WORD_LENGTH: usize = 5;

#[tokio::main]
async fn main() {
    println!("Starting server...");

    let server_status = String::from("Server is running.");
    let health = warp::path!()
        .and(warp::get())
        .map(move || warp::reply::json(&server_status));

    let region_provider = RegionProviderChain::default_provider().or_else(DEFAULT_AWS_REGION);
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&shared_config);

    let get_current_game = warp::path!("users" / String / "games" / "current")
        .and(warp::get())
        .and(with_dynamo_db(client.clone()))
        .and_then(get_current_game_handler);

    let new_game = warp::path!("users" / String / "games")
        .and(warp::post())
        .and(warp::body::content_length_limit(10))
        .and(warp::body::json())
        .and(with_dynamo_db(client.clone()))
        .and_then(new_game_handler);

    let guess = warp::path!("users" / String / "games" / "current" / "guesses")
        .and(warp::post())
        .and(warp::body::content_length_limit(1024))
        .and(warp::body::json())
        .and(with_dynamo_db(client.clone()))
        .and_then(guess_handler);

    let routes = health
        .or(get_current_game)
        .or(new_game)
        .or(guess)
        .with(warp::cors().allow_any_origin())
        .recover(handle_error);
    warp::serve(routes).run(([0, 0, 0, 0], 3030)).await;
}

/**
 * Middlewares
 */

fn with_dynamo_db(
    db_client: Client,
) -> impl Filter<Extract = (Client,), Error = Infallible> + Clone {
    warp::any().map(move || db_client.clone())
}

/**
 * Handlers
 */

async fn new_game_handler(
    user_id: String,
    _: NewGameRequest,
    client: Client,
) -> Result<Json, Rejection> {
    let game = new_game(user_id, client).await;
    Ok(warp::reply::json(&game))
}

async fn get_current_game_handler(user_id: String, client: Client) -> Result<Json, Rejection> {
    match get_current_game(user_id, &client).await {
        Some(game) => Ok(warp::reply::json(&game)),
        None => Err(warp::reject::not_found()),
    }
}

async fn guess_handler(user_id: String, guess: Guess, client: Client) -> Result<Json, Rejection> {
    match check_guess(user_id, guess, &client).await {
        Some(guess_result) => Ok(warp::reply::json(&guess_result)),
        None => Err(warp::reject::not_found()),
    }
}

/**
 * Database layer
 */
async fn new_game(user_id: String, client: Client) -> Game {
    let user_id_av = AttributeValue::S(user_id.clone().into());
    let index = choose_random_index();
    let chosen_word = words::WORDS[index];
    let chosen_word_av = AttributeValue::S(chosen_word.into());
    let guesses_av = AttributeValue::L(Vec::new());
    let request = client
        .put_item()
        .table_name(TABLE_NAME)
        .item("user_id", user_id_av)
        .item("word", chosen_word_av)
        .item("guesses", guesses_av);
    let _ = request.send().await.unwrap();

    let game = Game {
        user_id: user_id,
        word: String::from(chosen_word),
        guesses: Vec::new(),
    };
    game
}

/**
 * Current implementation is not really random. We're taking current  Couldn't get Rand to work with mutex lock and thread safety.
 */
fn choose_random_index() -> usize {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    nanos as usize % words::WORD_COUNT
}

async fn get_current_game(user_id: String, client: &Client) -> Option<Game> {
    let user_id_av = AttributeValue::S(user_id.clone());
    let request = client
        .get_item()
        .table_name(TABLE_NAME)
        .key("user_id", user_id_av);
    match request.send().await {
        Ok(get_item_output) => match get_item_output.item {
            Some(item) => Some(process_found_item(item)),
            None => None,
        },
        Err(_) => None,
    }
}

fn process_found_item(item: HashMap<String, AttributeValue>) -> Game {
    let user_id = String::from(item.get("user_id").unwrap().as_s().unwrap());
    let word = String::from(item.get("word").unwrap().as_s().unwrap());
    let guesses = item
        .get("guesses")
        .unwrap()
        .as_l()
        .unwrap()
        .iter()
        .map(|guess| {
            guess
                .as_l()
                .unwrap()
                .iter()
                .map(|attr_av| {
                    let attr: &HashMap<String, AttributeValue> = attr_av.as_m().unwrap();
                    let index = attr
                        .get("index")
                        .unwrap()
                        .as_n()
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    let status =
                        CharacterMatchStatus::from_str(attr.get("status").unwrap().as_s().unwrap())
                            .unwrap();
                    CharacterMatchResult {
                        index: index,
                        status: status,
                    }
                })
                .collect()
        })
        .collect();
    let game = Game {
        user_id: user_id,
        word: word,
        guesses: guesses,
    };
    game
}

async fn check_guess(user_id: String, guess: Guess, client: &Client) -> Option<GuessResult> {
    match get_current_game(user_id.clone(), client).await {
        Some(game) => {
            if game.guesses.len() >= MAX_GUESSES {
                return Some(GuessResult {
                    status: GuessStatus::GameOver,
                    place_matches: Vec::new(),
                });
            }
            match words::WORDS.iter().position(|&s| s == guess.guess) {
                Some(_) => {
                    Some(check_individual_characters(guess, game, user_id.clone(), client).await)
                }
                None => Some(process_invalid_guess(guess)),
            }
        }
        None => None,
    }
}

async fn check_individual_characters(
    guess: Guess,
    game: Game,
    user_id: String,
    client: &Client,
) -> GuessResult {
    let game_word_chars: Vec<char> = game.word.chars().collect();
    let guess_word_chars: Vec<char> = guess.guess.chars().collect();
    let mut char_match_results: Vec<CharacterMatchResult> = Vec::new();
    let mut exact_matches_count: usize = 0;
    for (i, guess_word_char) in guess_word_chars.iter().enumerate() {
        if *guess_word_char == game_word_chars[i] {
            char_match_results.push(CharacterMatchResult {
                index: i,
                status: CharacterMatchStatus::PresentAtCorrectPlace,
            });
            exact_matches_count += 1;
        } else {
            for (j, game_word_char) in game_word_chars.iter().enumerate() {
                if i != j && *guess_word_char == *game_word_char {
                    char_match_results.push(CharacterMatchResult {
                        index: i,
                        status: CharacterMatchStatus::PresentAtIncorrectPlace,
                    });
                    break;
                }
            }
            if char_match_results.len() < i + 1 {
                char_match_results.push(CharacterMatchResult {
                    index: i,
                    status: CharacterMatchStatus::NotPresent,
                });
            }
        }
    }
    let user_id_av = AttributeValue::S(user_id.into());

    let char_match_results_av: Vec<AttributeValue> = char_match_results
        .iter()
        .map(|match_result| {
            let mut attr_map: HashMap<String, AttributeValue> = HashMap::new();
            attr_map.insert(
                String::from("index"),
                AttributeValue::N(match_result.index.to_string()),
            );
            attr_map.insert(
                String::from("status"),
                AttributeValue::S(match_result.status.to_string()),
            );
            AttributeValue::M(attr_map)
        })
        .collect();
    let mut new_guess: Vec<AttributeValue> = Vec::new();
    new_guess.push(AttributeValue::L(char_match_results_av));
    let new_guess_av = AttributeValue::L(new_guess);
    let request = client
        .update_item()
        .table_name(TABLE_NAME)
        .key("user_id", user_id_av)
        .update_expression("SET #col = list_append(#col, :vals)")
        .expression_attribute_names("#col", "guesses")
        .expression_attribute_values(":vals", new_guess_av);
    let _ = request.send().await.unwrap();
    let guess_status: GuessStatus = match exact_matches_count == WORD_LENGTH {
        true => GuessStatus::PlayerWon,
        false => match game.guesses.len() == (MAX_GUESSES - 1) {
            true => GuessStatus::GameOver,
            false => GuessStatus::Evaluated,
        },
    };
    GuessResult {
        status: guess_status,
        place_matches: char_match_results,
    }
}

fn process_invalid_guess(guess: Guess) -> GuessResult {
    let length = guess.guess.len();
    let mut char_match_results: Vec<CharacterMatchResult> = Vec::new();
    for i in 0..length {
        char_match_results.push(CharacterMatchResult {
            index: i,
            status: CharacterMatchStatus::Invalid,
        });
    }
    GuessResult {
        status: GuessStatus::Invalid,
        place_matches: char_match_results,
    }
}

/**
 * Types for serialization and deserialization
 */

#[derive(Deserialize, Debug, Serialize)]
struct NewGameRequest {}

#[derive(Deserialize, Debug, Serialize)]
struct Game {
    pub user_id: String,
    pub word: String,
    pub guesses: Vec<Vec<CharacterMatchResult>>,
}

#[derive(Deserialize, Debug, Serialize, Validate)]
struct Guess {
    #[validate(length(min = 5, max = 5))]
    pub guess: String,
}

#[derive(Deserialize, Debug, Serialize)]
struct GuessResult {
    pub status: GuessStatus,
    pub place_matches: Vec<CharacterMatchResult>,
}

#[derive(Deserialize, Debug, Serialize)]
enum GuessStatus {
    Evaluated,
    Invalid,
    GameOver,
    PlayerWon,
}

#[derive(Deserialize, Debug, Serialize)]
struct CharacterMatchResult {
    pub index: usize,
    pub status: CharacterMatchStatus,
}

#[derive(Deserialize, Debug, Serialize, EnumString)]
enum CharacterMatchStatus {
    PresentAtCorrectPlace,
    PresentAtIncorrectPlace,
    NotPresent,
    Invalid,
}

impl fmt::Display for CharacterMatchStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CharacterMatchStatus::PresentAtCorrectPlace => write!(f, "PresentAtCorrectPlace"),
            CharacterMatchStatus::PresentAtIncorrectPlace => write!(f, "PresentAtIncorrectPlace"),
            CharacterMatchStatus::NotPresent => write!(f, "NotPresent"),
            CharacterMatchStatus::Invalid => write!(f, "Invalid"),
        }
    }
}

/**
 * Error types and handlers
 */

#[derive(Serialize)]
struct ErrorResponse {
    message: String,
}

pub async fn handle_error(err: Rejection) -> std::result::Result<impl Reply, Infallible> {
    let code;
    let message;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        message = "Not Found";
    } else if let Some(_) = err.find::<warp::filters::body::BodyDeserializeError>() {
        code = StatusCode::BAD_REQUEST;
        message = "Invalid Body";
    } else if let Some(e) = err.find::<CustomError>() {
        match e {
            CustomError::InvalidQuery => {
                code = StatusCode::BAD_REQUEST;
                message = "Please check your params";
            }
            CustomError::DBError => {
                code = StatusCode::INTERNAL_SERVER_ERROR;
                message = "Failed to query DB";
            }
        }
    } else if let Some(_) = err.find::<warp::reject::MethodNotAllowed>() {
        code = StatusCode::METHOD_NOT_ALLOWED;
        message = "Method Not Allowed";
    } else if let Some(_) = err.find::<warp::reject::InvalidQuery>() {
        code = StatusCode::BAD_REQUEST;
        message = "Please check your params";
    } else {
        eprintln!("unhandled error: {:?}", err);
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "Internal Server Error";
    }

    let json = warp::reply::json(&ErrorResponse {
        message: message.into(),
    });

    Ok(warp::reply::with_status(json, code))
}

#[derive(Error, Debug)]
pub enum CustomError {
    #[error("Invalid query params")]
    InvalidQuery,
    #[error("Failed to query DB")]
    DBError,
}

impl warp::reject::Reject for CustomError {}
