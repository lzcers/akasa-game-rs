# syntax=docker/dockerfile:1.7
FROM rust:1.96.0-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace/akashic

COPY alaya-agent-rs /workspace/alaya-agent-rs
COPY akashic/Cargo.toml akashic/Cargo.lock ./
COPY akashic/story-engine ./story-engine
COPY akashic/server ./server

RUN --mount=type=secret,id=cargo_config,target=/run/secrets/cargo_config,required=false \
    if [ -f /run/secrets/cargo_config ]; then \
    mkdir -p /usr/local/cargo && \
    cp /run/secrets/cargo_config /usr/local/cargo/config.toml; \
    fi && \
    cargo build --release -p akasa-server && \
    rm -f /usr/local/cargo/config.toml

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /workspace/akashic/target/release/akasa-server /usr/local/bin/akasa-server

EXPOSE 3001

CMD ["akasa-server"]
