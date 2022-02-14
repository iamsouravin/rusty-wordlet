use std::thread::sleep;
use std::time::{Duration, SystemTime};

fn main() {
    println!("Hello, world!");

    loop {
        let now = SystemTime::now();

        sleep(Duration::new(2, 0));
        match now.elapsed() {
            Ok(elapsed) => {
                println!("Slept for {} seconds.", elapsed.as_secs());
            }
            Err(e) => {
                println!("Error: {:?}", e);
            }
        }
        println!("")
    }
}
