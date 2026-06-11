FROM rust:1.96-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY config.toml ./

RUN cargo build --release

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/relic-monitor-backend /app/relic-monitor-backend
COPY --from=builder /app/config.toml /app/config.toml
COPY --from=builder /app/frontend /app/frontend

ENV RUST_LOG=info,relic_monitor_backend=debug
ENV SERVER_HOST=0.0.0.0
ENV SERVER_PORT=8080
ENV FRONTEND_DIR=/app/frontend

EXPOSE 8080
EXPOSE 9090

CMD ["/app/relic-monitor-backend"]
