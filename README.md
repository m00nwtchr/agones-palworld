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
./target/release/agones-palworld \
  --api-url http://127.0.0.1:8211 \
  --admin-password changeme \
  --pod-name test --pod-namespace default
```

The binary also accepts the same values via environment variables (CLI > env > default):

```bash
PALWORLD_API_URL=http://127.0.0.1:8211 \
PALWORLD_ADMIN_PASSWORD=changeme \
POD_NAME=test POD_NAMESPACE=default \
./target/release/agones-palworld
```

See `./target/release/agones-palworld --help` for the full flag list.

## Build the image

```bash
./scripts/build-image.sh ghcr.io/m00nwtchr/agones-palworld 0.1.0
```

The image builds against `x86_64-unknown-linux-musl` and runs from `scratch` with
ca-certificates and a `UID 999` passwd entry; see `Dockerfile`.

## Helm install

```bash
helm install palworld ./helm \
  --namespace games --create-namespace \
  --set palworld.image.tag=v1.0.1.100619@sha256:0d293cafd503a91a6d11d71f7bf770ee0c3c5ecf37db988349b2c1758f4e9358 \
  --set sidecar.image.tag=0.1.0@sha256:<digest-pinned-by-ci>
```

### Install from GHCR OCI registry

```bash
helm install palworld oci://ghcr.io/m00nwtchr/charts/palworld \
  --version 0.1.0 \
  --set palworld.image.tag=v1.0.1.100619@sha256:0d293cafd503a91a6d11d71f7bf770ee0c3c5ecf37db988349b2c1758f4e9358
```

The sidecar image tag defaults to the chart's appVersion via the chart's `default` template helper; pin the digest via Flux for production.

See `helm/README.md` for chart-specific overrides and the UID/GID rationale.
