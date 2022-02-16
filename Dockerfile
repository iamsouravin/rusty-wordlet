FROM rust:1.58.1-buster as builder

RUN apt-get install -y --no-install-recommends ca-certificates \
    && update-ca-certificates

RUN USER=root cargo new --bin rusty_wordlet
WORKDIR /rusty_wordlet
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --release
RUN rm src/*.rs

ADD . ./

RUN rm ./target/release/deps/rusty_wordlet*

RUN cargo build --release

FROM debian:buster-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && update-ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /rusty_wordlet/target/release/rusty_wordlet /usr/local/bin/rusty_wordlet

CMD ["rusty_wordlet"]