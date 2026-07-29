# syntax=docker/dockerfile:1.7
FROM clux/muslrust:stable AS chef
USER root
RUN cargo install --locked cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json

RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

RUN printf '#!/bin/sh\nexec /usr/bin/protoc -I/usr/include "$@"\n' > /usr/local/bin/protoc \
    && chmod +x /usr/local/bin/protoc

RUN cargo chef cook --release --target x86_64-unknown-linux-musl --recipe-path recipe.json
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl --bin agones-palworld

FROM scratch AS runtime
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/agones-palworld /
USER 999:999
CMD ["/agones-palworld"]
