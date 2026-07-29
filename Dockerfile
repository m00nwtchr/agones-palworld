# syntax=docker/dockerfile:1.7
FROM rust:bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && \
    echo 'fn main() { println!("dummy"); }' > src/main.rs && \
    echo '' > src/lib.rs && \
    cargo build --release --locked && \
    rm -rf src target/release/deps/agones_palworld*

COPY src ./src
RUN touch src/main.rs && cargo build --release --locked && \
    strip target/release/agones-palworld

FROM gcr.io/distroless/cc-debian12
COPY --from=builder /build/target/release/agones-palworld /agones-palworld
EXPOSE 9090
ENTRYPOINT ["/agones-palworld"]