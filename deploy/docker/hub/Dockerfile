# syntax=docker/dockerfile:1

FROM rust:1.83-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/lan-iot-hub /usr/local/bin/lan-iot-hub
EXPOSE 3000
ENV RUST_LOG=lan_iot_hub=info,tower_http=info
CMD ["lan-iot-hub"]
