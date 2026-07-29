# syntax=docker/dockerfile:1.7
FROM rust:bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    musl-tools pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
RUN echo 'palworld:x:999:999:palworld:/:/sbin/nologin' >> /etc/passwd
ENV RUSTFLAGS="-C target-feature=+crt-static"
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && \
    echo 'fn main() {}' > src/main.rs && \
    echo '' > src/lib.rs && \
    cargo build --release --target x86_64-unknown-linux-musl --locked && \
    rm -rf src target/x86_64-unknown-linux-musl/release/deps/agones_palworld*
COPY src ./src
RUN touch src/main.rs && \
    cargo build --release --target x86_64-unknown-linux-musl --locked && \
    strip target/x86_64-unknown-linux-musl/release/agones-palworld

FROM scratch
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/agones-palworld /agones-palworld
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
USER 999:999
EXPOSE 9090
ENTRYPOINT ["/agones-palworld"]