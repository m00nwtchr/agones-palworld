# agones-palworld

Agones SDK sidecar for the Palworld dedicated server, plus a Helm chart that
provisions a single-pod Fleet.

See `docs/superpowers/specs/2026-07-29-agones-palworld-sidecar-design.md` for
the full design.

## Layout

- `src/` — Rust sidecar binary
- `Dockerfile` — multi-stage distroless image
- `helm/` — Helm chart
- `docs/superpowers/` — design spec and implementation plan

## Build

```bash
devenv shell
cargo build --release
```

## Test

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
```

## Run locally

```bash
PALWORLD_API_URL=http://localhost:8211 \
PALWORLD_ADMIN_PASSWORD=changeme \
POD_NAME=test POD_NAMESPACE=default \
./target/release/agones-palworld
```

## Build the image

```bash
./scripts/build-image.sh ghcr.io/m00nwtchr/agones-palworld 0.1.0
```

## Helm install

```bash
helm install palworld ./helm \
  --namespace games --create-namespace \
  --set palworld.image.tag=v1.0.1.100619@sha256:0d293cafd503a91a6d11d71f7bf770ee0c3c5ecf37db988349b2c1758f4e9358
```
