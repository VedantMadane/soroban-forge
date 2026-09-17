FROM rust:1.75-slim AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32v1-none

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY tests ./tests

RUN cargo build --workspace --release --target wasm32v1-none

FROM scratch
COPY --from=builder /app/target/wasm32v1-none/release/*.wasm /wasm/
