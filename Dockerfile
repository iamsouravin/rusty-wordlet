FROM rust:1.58.1-buster as builder

ARG ARCH=x86_64-unknown-linux-gnu

RUN USER=root cargo new --bin rusty_wordlet
WORKDIR /rusty_wordlet
COPY ./Cargo.toml ./Cargo.toml
RUN cargo build --release --target ${ARCH}
RUN rm src/*.rs

ADD . ./

RUN rm ./target/${ARCH}/release/deps/rusty_wordlet*

RUN RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --target ${ARCH}

FROM scratch

ARG ARCH=x86_64-unknown-linux-gnu

COPY --from=builder /rusty_wordlet/target/${ARCH}/release/rusty_wordlet /rusty_wordlet
CMD ["/rusty_wordlet"]