# syntax=docker/dockerfile:1.7
FROM rust:stable-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked && strip target/release/agones-palworld

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /build/target/release/agones-palworld /agones-palworld
EXPOSE 9090
ENTRYPOINT ["/agones-palworld"]
