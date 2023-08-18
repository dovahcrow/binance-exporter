FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release

FROM ubuntu:20.04 as runtime

RUN apt-get update
RUN apt-get install --no-install-recommends -y curl ca-certificates libssl-dev && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/binance-exporter /usr/local/bin/binance-exporter

EXPOSE 9090

ENV RUST_LOG=binance_exporter=info

ENTRYPOINT binance-exporter
