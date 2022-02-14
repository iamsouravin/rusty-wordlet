FROM rust:1.58.1-buster as builder

RUN USER=root cargo new --bin rusty_wordlet
WORKDIR /rusty_wordlet
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --release
RUN rm src/*.rs

ADD . ./

RUN rm ./target/release/deps/rusty_wordlet*
RUN cargo build --release

FROM scratch

COPY --from=builder /rusty_wordlet/target/release/rusty_wordlet /rusty_wordlet
CMD ["/rusty_wordlet"]