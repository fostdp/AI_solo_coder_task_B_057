FROM rust:1.96-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/lora-simulator /app/lora-simulator

ENV RUST_LOG=info,lora_simulator=debug
ENV SERVER_URL=http://backend:8080
ENV SAMPLE_INTERVAL_SECS=1800
ENV DEVICE_COUNT=80
ENV BATCH_MODE=true

EXPOSE 9091

CMD ["/app/lora-simulator"]
