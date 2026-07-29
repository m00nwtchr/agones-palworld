# agones-palworld round 2 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Address five round-2 adjustments (clap config, `FROM scratch` musl runtime, `/healthz` endpoint, dualstack metrics bind, single image tag) plus two clarifications (UID/GID 999 + IPv4-only game service).

**Architecture:** Six focused changes — clap-derive on `Config`, an HTTP `/healthz` handler next to `/metrics`, multi-stage Dockerfile ending in `FROM scratch` with musl, restructured Helm values (no separate digest field), UID 999 propagated through pod/containers, and the game UDP service pinned to IPv4 while the metrics service is dualstack. Round-1 behavior (Agones SDK bridge, polling loop, etc.) is preserved.

**Tech Stack:** Existing (clap 4 + tokio + reqwest + agones 1.59 + opentelemetry 0.27) **plus** `clap = { version = "4", features = ["derive", "env"] }`. Build target: `x86_64-unknown-linux-musl`. Runtime: `FROM scratch`.

## Global Constraints

- Rust edition 2024, channel stable, MSRV tracks latest stable (1.97+).
- License MPL-2.0.
- All work happens in the worktree at `~/.worktrees/round-2-clap-scratch/`.
- Quality gates before every commit: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `helm lint helm --set palworld.image.tag="v0.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000"`, `shellcheck scripts/*.sh`.
- Use `devenv shell -- bash -c '...'` for every cargo/helm/shellcheck command.
- No comments in source code unless explicitly required for safety.
- Git identity: do NOT override committer identity. If GPG signing fails, pass `-c commit.gpgsign=false` for that ONE commit only.
- `palworld` REST API URL is IPv4 (`http://127.0.0.1:8211`); the sidecar listens dualstack but connects to the game server via v4 literal only.

---

## Task 1: clap derive on Config

**Files:**
- Modify: `Cargo.toml` — add `clap`
- Modify: `src/config.rs` — full rewrite to use clap derive; remove env-var helpers
- Modify: `src/main.rs` — call `Config::load()` instead of `Config::from_env()`

**Interfaces:**
- Produces: `pub struct Config { ... }` with `#[derive(Parser, Debug)]`, `Config::load() -> AppResult<Self>`. Default values match round 1 except `metrics_host` defaults to `"::"`.

- [ ] **Step 1.1: Add clap dep to Cargo.toml**

Append under `[dependencies]`:

```toml
clap = { version = "4", features = ["derive", "env"] }
```

Run: `devenv shell -- bash -c 'cargo check --all-targets 2>&1 | tail -10'`
Expected: compiles — clap fetches.

- [ ] **Step 1.2: Rewrite `src/config.rs`**

Replace the entire content of `src/config.rs` with:

```rust
#![allow(unsafe_code)]
#![allow(clippy::result_large_err)]

use std::time::Duration;

use clap::Parser;
use url::Url;

use crate::error::AppResult;

#[derive(Debug)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        use std::ptr;
        unsafe {
            let bytes = self.0.as_mut_ptr();
            for i in 0..self.0.len() {
                ptr::write_volatile(bytes.add(i), 0);
            }
            self.0.clear();
        }
    }
}

#[derive(Parser, Debug)]
#[command(version, about = "Agones sidecar for Palworld dedicated server")]
pub struct Config {
    #[arg(long, env = "PALWORLD_API_URL", default_value = "http://127.0.0.1:8211")]
    pub api_url: Url,
    #[arg(long, env = "PALWORLD_ADMIN_PASSWORD")]
    pub admin_password: SecretString,
    #[arg(long, env = "POLL_INTERVAL_SECS", default_value_t = 5)]
    pub poll_interval_secs: u64,
    #[arg(long, env = "HEALTH_INTERVAL_SECS", default_value_t = 2)]
    pub health_interval_secs: u64,
    #[arg(long, env = "SHUTDOWN_SAVE_TIMEOUT_SECS", default_value_t = 30)]
    pub shutdown_save_timeout_secs: u64,
    #[arg(long, env = "SHUTDOWN_WAITTIME_SECS", default_value_t = 30)]
    pub shutdown_waittime_secs: u32,
    #[arg(long, env = "SHUTDOWN_ANNOUNCE_MESSAGE", default_value = "Server shutting down")]
    pub shutdown_announce: String,
    #[arg(long, env = "METRICS_PORT", default_value_t = 9090)]
    pub metrics_port: u16,
    #[arg(long, env = "METRICS_HOST", default_value = "::")]
    pub metrics_host: String,
    #[arg(long, env = "DISABLE_PROMETHEUS", default_value_t = false)]
    pub disable_prometheus: bool,
    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    pub otel_endpoint: Option<String>,
    #[arg(long, env = "POD_NAME", default_value = "unknown")]
    pub pod_name: String,
    #[arg(long, env = "POD_NAMESPACE", default_value = "default")]
    pub pod_namespace: String,
}

impl Config {
    pub fn load() -> AppResult<Self> {
        let mut cfg = <Self as Parser>::parse();
        if cfg.api_url.host_str() == Some("localhost") {
            tracing::warn!("api_url uses localhost; prefer 127.0.0.1 to avoid IPv6 lookups on dualstack");
        }
        if let Ok(s) = std::env::var("SHUTDOWN_ANNOUNCE_MESSAGE") {
            cfg.shutdown_announce = s;
        }
        Ok(cfg)
    }
}

impl Clone for SecretString {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
```

Note: `SecretString` keeps its drop impl (defense in depth) and gains `Clone` because `Config` derives `Debug` and clap sometimes needs `Clone`. The password is moved in once via `PALWORLD_ADMIN_PASSWORD`; subsequent debug prints show only the header struct fields, never the password.

The `if let Ok(s) = std::env::var(...)` line is a deliberate fallback in case the user's parent shell has `SHUTDOWN_ANNOUNCE_MESSAGE` already set when they remove the default_value behavior later. If you object to that fallback, delete it — not required.

- [ ] **Step 1.3: Rewrite the inline tests**

Replace the `#[cfg(test)] mod tests { ... }` block in `src/config.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_api_url_via_cli_or_env() {
        let err = Config::try_parse_from(["agones-palworld"]).unwrap_err();
        assert!(
            err.to_string().contains("PALWORLD_API_URL"),
            "got: {err}"
        );
    }

    #[test]
    fn reads_all_values_from_cli() {
        let c = Config::try_parse_from([
            "agones-palworld",
            "--api-url", "http://127.0.0.1:8211",
            "--admin-password", "hunter2",
            "--poll-interval-secs", "5",
            "--metrics-port", "9090",
            "--pod-name", "palworld-0",
            "--pod-namespace", "games",
        ])
        .expect("config");
        assert_eq!(c.api_url.as_str(), "http://127.0.0.1:8211/");
        assert_eq!(c.poll_interval_secs, 5);
        assert_eq!(c.metrics_port, 9090);
        assert_eq!(c.pod_name, "palworld-0");
        assert_eq!(c.pod_namespace, "games");
        assert_eq!(c.metrics_host, "::");
        assert!(c.otel_endpoint.is_none());
        assert!(!c.disable_prometheus);
    }

    #[test]
    fn env_vars_override_defaults() {
        // clap will read env vars automatically because of `env = "..."`
        // but `try_parse_from` doesn't see them. Use `parse_from` instead
        // and verify env is wired:
        std::env::set_var("PALWORLD_API_URL", "http://127.0.0.1:8211");
        std::env::set_var("PALWORLD_ADMIN_PASSWORD", "hunter2");
        let c = Config::parse_from(["agones-palworld"]).expect("config");
        assert_eq!(c.api_url.as_str(), "http://127.0.0.1:8211/");
        assert_eq!(c.metrics_host, "::");
        std::env::remove_var("PALWORLD_API_URL");
        std::env::remove_var("PALWORLD_ADMIN_PASSWORD");
    }

    #[test]
    fn cli_args_override_env_vars() {
        std::env::set_var("PALWORLD_API_URL", "http://127.0.0.1:8211");
        std::env::set_var("PALWORLD_ADMIN_PASSWORD", "env-pw");
        let c = Config::parse_from([
            "agones-palworld",
            "--admin-password", "cli-pw",
            "--api-url", "http://127.0.0.1:8211",
        ])
        .expect("config");
        assert_eq!(c.admin_password.expose(), "cli-pw");
        std::env::remove_var("PALWORLD_API_URL");
        std::env::remove_var("PALWORLD_ADMIN_PASSWORD");
    }
}
```

Note: these tests don't use the `ENV_LOCK` Mutex from round 1 because clap's `parse_from`/`try_parse_from` builds the config from explicit args or directly from the live env, so they don't race against other tests. If you find a race in CI, add the Mutex back.

- [ ] **Step 1.4: Update `src/main.rs` line that builds Config**

Find:
```rust
let cfg = Config::from_env()?;
```
Replace with:
```rust
let cfg = Config::load()?;
```

- [ ] **Step 1.5: Verify quality gates**

```bash
devenv shell -- bash -c 'cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test'
```

Expected: 4 config tests + round 1 tests all green. Clippy clean.

- [ ] **Step 1.6: Commit**

```bash
git add Cargo.toml src/config.rs src/main.rs
git -c commit.gpgsign=false commit -m "feat(config): clap derive parser; CLI > env > default precedence"
```

---

## Task 2: `metrics_host` default to `::` (dualstack bind)

**Files:**
- Modify: `helm/values.yaml`
- Modify: `src/config.rs` (already updated in Task 1 — verify it says `"::"` not `"0.0.0.0"`)
- Modify: `docs/.../round-2 spec` only if it references old default

**Interfaces:**
- Default of `Config::metrics_host` should be `"::"` (already in Task 1).
- Helm values `metrics.service.ipFamilyPolicy` defaults to `PreferDualStack`.

- [ ] **Step 2.1: Verify Task 1's default is `::`**

Run: `grep -n 'metrics_host' src/config.rs`
Expected: shows `default_value = "::"`. If not, edit Task 1 to add it.

- [ ] **Step 2.2: Update `helm/values.yaml`**

In the `metrics.service:` block, add (after `type:`):
```yaml
    ipFamilyPolicy: PreferDualStack
    ipFamilies: [IPv6, IPv4]
```

- [ ] **Step 2.3: Update `helm/templates/metrics-service.yaml`**

Find the `spec:` block in `metrics-service.yaml` and add:
```yaml
  ipFamilyPolicy: {{ .Values.metrics.service.ipFamilyPolicy }}
  ipFamilies:
  {{- toYaml .Values.metrics.service.ipFamilies | nindent 4 }}
```

If the file currently doesn't have these, add them.

- [ ] **Step 2.4: Verify `helm template` renders dualstack Service**

```bash
devenv shell -- bash -c 'helm template test ./helm \
  --set palworld.image.tag=v0.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000 2>&1 | grep -A 10 metrics-service'
```

Expected: shows `ipFamilyPolicy: PreferDualStack` and `ipFamilies:\n    - IPv6\n    - IPv4`.

- [ ] **Step 2.5: Commit**

```bash
git add helm/values.yaml helm/templates/metrics-service.yaml
git -c commit.gpgsign=false commit -m "feat(metrics): dualstack IPv4+IPv6 bind on [::] for the metrics service"
```

---

## Task 3: PALWORLD_API_URL defaults to IPv4 literal

**Files:**
- Modify: `src/config.rs` (done in Task 1 — `default_value = "http://127.0.0.1:8211"`)
- Modify: `helm/templates/fleet.yaml` — set the env var explicitly to the chart value

**Interfaces:**
- Sidecar always connects to `http://127.0.0.1:8211` by default. Chart overrides via `palworld.restPort` (or env var override path).

- [ ] **Step 3.1: Verify Task 1 default**

```bash
grep -n 'PALWORLD_API_URL' src/config.rs
```
Expected: shows `default_value = "http://127.0.0.1:8211"`.

- [ ] **Step 3.2: Update Fleet template PALWORLD_API_URL wiring**

In `helm/templates/fleet.yaml`, find the sidecar container's `env:` block. The line currently says:
```yaml
- name: PALWORLD_API_URL
  value: "http://localhost:{{ .Values.palworld.restPort }}"
```
Replace `localhost` with `127.0.0.1`:
```yaml
- name: PALWORLD_API_URL
  value: "http://127.0.0.1:{{ .Values.palworld.restPort }}"
```

- [ ] **Step 3.3: Commit**

```bash
git add helm/templates/fleet.yaml
git -c commit.gpgsign=false commit -m "fix(net): use 127.0.0.1 IPv4 literal for PALWORLD_API_URL — game server is v4 only"
```

---

## Task 4: `/healthz` endpoint + cached background probe

**Files:**
- Modify: `src/observability.rs` — add `palworld_reachable` AtomicU8, background probe task, `/healthz` handler

**Interfaces:**
- `Metrics` gains `pub palworld_health: Arc<AtomicU8>` and a `HealthSnapshot` type alias for clarity.
- `install()` spawns a background task that calls `client.info()` every 5 s and updates the atomic.
- HTTP handler routes `GET /healthz` to a new function returning 200 or 503.

- [ ] **Step 4.1: Add `AtomicU8` health state to `Metrics`**

In `src/observability.rs`, find the `Metrics` struct. Add near the top of the struct:

```rust
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

pub const HEALTH_UNKNOWN: u8 = 0;
pub const HEALTH_OK: u8 = 1;
pub const HEALTH_BAD: u8 = 2;

pub struct Metrics {
    pub palworld_health: Arc<AtomicU8>,
    pub poll_cycles: Counter<u64>,
    // ... rest of fields
}

impl Clone for Metrics {
    fn clone(&self) -> Self {
        Self {
            palworld_health: self.palworld_health.clone(),
            poll_cycles: self.poll_cycles.clone(),
            // ... clone each remaining field
        }
    }
}
```

If `Metrics` already implements `Clone`, leave it and just add the field. If implementing it for the first time, follow the same pattern.

Then in the `install()` function where `Metrics` is constructed, add:
```rust
let palworld_health = Arc::new(AtomicU8::new(HEALTH_UNKNOWN));
m.palworld_health = palworld_health.clone();
```

Or whatever assignment shape your existing code uses.

- [ ] **Step 4.2: Spawn the background probe task in `install()`**

In `install()`, after the meter provider is set and the metric instruments are built, add:

```rust
let probe_state = m.palworld_health.clone();
let probe_client = palworld::Client::new(cfg.api_url.clone(), cfg.admin_password.expose());
let probe_handle = tokio::spawn(async move {
    let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
    loop {
        ticker.tick().await;
        let state = match probe_client.info().await {
            Ok(_) => HEALTH_OK,
            Err(_) => HEALTH_BAD,
        };
        probe_state.store(state, Ordering::Relaxed);
    }
});
```

You need to wire `probe_handle` so that `Guard::drop` aborts the task. Adjust `Guard`:

```rust
pub struct Guard {
    _provider: SdkMeterProvider,
    pub registry: Registry,
    server_shutdown: tokio::sync::watch::Sender<bool>,
    health_probe: tokio::task::JoinHandle<()>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.server_shutdown.send(true);
        self.health_probe.abort();
        opentelemetry::global::shutdown_tracer_provider();
    }
}
```

- [ ] **Step 4.3: Add `/healthz` handler to the HTTP server**

In `src/observability.rs`, find `handle()` and add a branch before the path check:

```rust
async fn handle(
    req: Request<hyper::body::Incoming>,
    registry: &Registry,
    health: &AtomicU8,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if req.uri().path() == "/healthz" {
        let s = health.load(Ordering::Relaxed);
        let body = if s == HEALTH_OK { "{\"sidecar\":\"ok\",\"palworld\":\"ok\"}" }
                  else                  { "{\"sidecar\":\"ok\",\"palworld\":\"down\"}" };
        let status = if s == HEALTH_OK { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
        return Ok(Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .expect("static response"));
    }
    if req.method() != hyper::Method::GET || req.uri().path() != "/metrics" {
        // existing 404 path
    }
    // existing metrics encode path
}
```

Update the call site(s) of `handle(...)` to pass `health`. There's likely one: in the `service_fn` closure inside `run_metrics_server`.

- [ ] **Step 4.4: Write a test for `/healthz`**

Append to `src/observability.rs` (in the existing `#[cfg(test)] mod tests` block):

```rust
#[test]
fn healthz_reports_unknown_until_first_probe() {
    let health = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(HEALTH_UNKNOWN));
    // We can't easily test the full HTTP path without spinning up the server,
    // but the symbolic mapping is: HEALTH_OK -> 200, else -> 503.
    assert_eq!(health.load(std::sync::atomic::Ordering::Relaxed), HEALTH_UNKNOWN);
    health.store(HEALTH_OK, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(health.load(std::sync::atomic::Ordering::Relaxed), HEALTH_OK);
}
```

- [ ] **Step 4.5: Verify quality gates**

```bash
devenv shell -- bash -c 'cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test'
```
Expected: all green.

- [ ] **Step 4.6: Commit**

```bash
git add src/observability.rs
git -c commit.gpgsign=false commit -m "feat(observability): /healthz endpoint with cached palworld_reachable probe"
```

---

## Task 5: Dockerfile for musl + `FROM scratch` + UID 999

**Files:**
- Modify: `Dockerfile`

**Interfaces:**
- Multi-stage build: `rust:bookworm` builder with musl + UID 999 /etc/passwd entry → `FROM scratch` runtime.
- Build target: `x86_64-unknown-linux-musl`.
- Final image runs as USER 999:999.

- [ ] **Step 5.1: Replace `Dockerfile` with the musl + scratch version**

```dockerfile
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
```

- [ ] **Step 5.2: Optional: validate the build (if docker available)**

```bash
docker buildx build --load --tag agones-palworld:round-2 . 2>&1 | tail -10
```
Expected: completes with the musl target. Image tagged. If docker isn't available in this env, skip and note in the report.

- [ ] **Step 5.3: Commit**

```bash
git add Dockerfile
git -c commit.gpgsign=false commit -m "feat(docker): FROM scratch runtime with musl + UID 999 + ca-certs"
```

---

## Task 6: Helm — collapse `sidecar.image.digest` to single `image.tag`

**Files:**
- Modify: `helm/values.yaml`
- Modify: `helm/templates/_helpers.tpl`
- Modify: `helm/templates/fleet.yaml`

**Interfaces:**
- `sidecar.image.tag` defaults to `""`. Fleet template uses `{{ default .Chart.AppVersion .Values.X.image.tag }}` so chart version fills in when empty.
- `palworld.image.tag` is required (validation fires when empty).
- `image.repository` defaults preserved for both.

- [ ] **Step 6.1: Update `helm/values.yaml`**

In the `palworld.image:` block, keep `repository: ghcr.io/pocketpairjp/palserver` and `tag: ""`. Add a comment documenting the format.

In the `sidecar.image:` block, remove the `digest: ""` line entirely (and its `default .Chart.AppVersion` fallback). Keep `repository`, `tag`, `pullPolicy`.

- [ ] **Step 6.2: Update `helm/templates/_helpers.tpl`**

Find the validation rule that checked `.sidecar.image.digest`. Delete that rule. Update the remaining rules to check `.image.tag` (which now includes the optional digest). Update the error message wording.

The remaining rules should be:
```gotemplate
{{- if not .Values.palworld.image.tag -}}
{{- fail "palworld.image.tag is required (format: \"vX.X.X\" or \"vX.X.X@sha256:digest\")." -}}
{{- end -}}

{{- if not .Values.sidecar.image.tag -}}
{{- fail "sidecar.image.tag is required (defaults to chart appVersion; CI fills in the digest)." -}}
{{- end -}}
```

Adjust the surrounding rules (PALWORLD_RESTAPI_ENABLED guard, env-keys prefix check, etc.) to match — they're already in the file from round 1.

- [ ] **Step 6.3: Update `helm/templates/fleet.yaml`**

Find the sidecar image assembly block (the 3-way conditional from round 1's review fix). Replace with a single line:

```yaml
image: "{{ .Values.sidecar.image.repository }}:{{- default .Chart.AppVersion .Values.sidecar.image.tag -}}"
```

For the palworld container's image, similarly:
```yaml
image: "{{ .Values.palworld.image.repository }}:{{ .Values.palworld.image.tag }}"
```

- [ ] **Step 6.4: Verify `helm template`**

```bash
devenv shell -- bash -c 'helm template test ./helm \
  --set palworld.image.tag=v1.0.0 \
  --set sidecar.image.tag=1.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000 \
  | grep -E "image:|appVersion" | head -10'
```
Expected: shows the rendered images with the supplied tag and the digest pins.

- [ ] **Step 6.5: Verify the validation fires on empty `palworld.image.tag`**

```bash
devenv shell -- bash -c 'helm template test ./helm 2>&1 | head -3'
```
Expected: error message about missing `palworld.image.tag`.

- [ ] **Step 6.6: Commit**

```bash
git add helm/values.yaml helm/templates/_helpers.tpl helm/templates/fleet.yaml
git -c commit.gpgsign=false commit -m "feat(helm): collapse sidecar.image.digest into image.tag; chart version fallback"
```

---

## Task 7: Helm — UID 999 + IPv4-only game service

**Files:**
- Modify: `helm/values.yaml`
- Modify: `helm/templates/_helpers.tpl`
- Modify: `helm/templates/fleet.yaml`
- Modify: `helm/templates/service.yaml` (game UDP)

**Interfaces:**
- `pod.securityContext` and `containerSecurityContext` defaults expose UID 999 + runtime defaults.
- `service.ipFamilyPolicy: SingleStack` + `service.ipFamilies: [IPv4]` for the game UDP Service.

- [ ] **Step 7.1: Add pod + container SCC defaults to `helm/values.yaml`**

Add (after the `pvc:` block):
```yaml
pod:
  securityContext:
    runAsNonRoot: true
    runAsUser: 999
    runAsGroup: 999
    fsGroup: 999
    fsGroupChangePolicy: OnRootMismatch
    seccompProfile: { type: RuntimeDefault }

containerSecurityContext:
  allowPrivilegeEscalation: false
  readOnlyRootFilesystem: true
  capabilities: { drop: ["ALL"] }
```

Update `service:` block:
```yaml
service:
  type: Headless
  port: 8211
  protocol: UDP
  ipFamilyPolicy: SingleStack
  ipFamilies: [IPv4]
  annotations: { }
  labels: { }
```

- [ ] **Step 7.2: Update `helm/templates/service.yaml` (game UDP)**

Replace the existing `spec:` body with:
```yaml
spec:
  type: {{ .Values.service.type }}
  ipFamilyPolicy: {{ .Values.service.ipFamilyPolicy }}
  ipFamilies:
  {{- toYaml .Values.service.ipFamilies | nindent 4 }}
  ports:
    - name: {{ .Values.service.protocol | lower }}
      port: {{ .Values.service.port }}
      targetPort: game
      protocol: {{ .Values.service.protocol }}
  selector:
    {{- include "agones-palworld.selectorLabels" . | nindent 4 }}
    agones.dev/fleet: {{ include "agones-palworld.fullname" . | quote }}
```

- [ ] **Step 7.3: Update Fleet template SCC wiring**

In `helm/templates/fleet.yaml`, find the `spec:` block of the GameServer template. Add at the top:
```yaml
  securityContext:
    {{- toYaml .Values.pod.securityContext | nindent 4 }}
```

Then in each container's body, add:
```yaml
  securityContext:
    {{- toYaml .Values.containerSecurityContext | nindent 4 }}
```

The exact indentation should match Kubernetes' spec — typically `securityContext` is a sibling of `name`, `image`, etc. inside the container.

- [ ] **Step 7.4: Verify `helm template`**

```bash
devenv shell -- bash -c 'helm template test ./helm \
  --set palworld.image.tag=v1.0.0 \
  --set sidecar.image.tag=1.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000 \
  | grep -E "runAsUser|runAsGroup|fsGroup|ipFamily|ipFamilies" | head -20'
```
Expected: shows `runAsUser: 999` etc. and `ipFamilyPolicy: SingleStack` for the game service.

- [ ] **Step 7.5: Verify `helm lint`**

```bash
devenv shell -- bash -c 'helm lint helm \
  --set palworld.image.tag=v1.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000 \
  --set sidecar.image.tag=1.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000'
```
Expected: clean (0 failed).

- [ ] **Step 7.6: Commit**

```bash
git add helm/values.yaml helm/templates/service.yaml helm/templates/fleet.yaml
git -c commit.gpgsign=false commit -m "feat(helm): UID 999 SCC defaults + IPv4-only game UDP service"
```

---

## Task 8: Update README + AGENTS.md

**Files:**
- Modify: `helm/README.md`
- Modify: `README.md`
- Modify: `AGENTS.md`

**Interfaces:**
- Helm README install example uses `image.tag` (single field).
- Root README mentions clap, scratch, dualstack, single-tag, UID 999.
- AGENTS.md gains the full superpowers dev flow convention per the user's instructions.

- [ ] **Step 8.1: Update `helm/README.md`**

In the install example, change `--set palworld.image.tag=...` to demonstrate the new format. Add a note: "Pin the sidecar image tag with `@sha256:digest` via CI for immutability." Add a section "UID/GID 999" explaining the why.

- [ ] **Step 8.2: Update root `README.md`**

Update the dev / install sections:
- "Build": use `devenv shell` for `cargo` (already there)
- "Run locally": show the clap CLI form, e.g. `./target/release/agones-palworld --api-url http://127.0.0.1:8211 --admin-password changeme`
- "Build the image": `./scripts/build-image.sh ...` (no change needed; build script is unchanged)
- "Helm install": show the new `palworld.image.tag` format

- [ ] **Step 8.3: Append the full superpowers dev flow to `AGENTS.md`**

Append:
```markdown

## Full development flow

**Always run the entire superpowers dev flow for every round of changes:**

- **brainstorming** — Activates before writing code. Refines rough ideas through questions, explores alternatives, presents design in sections for validation. Saves design document to `docs/superpowers/specs/`.
- **using-git-worktrees** — Activates after design approval. Creates isolated workspace on a new branch, runs project setup, verifies clean test baseline.
- **writing-plans** — Activates with approved design. Breaks work into bite-sized tasks (2–5 minutes each). Every task has exact file paths, complete code, verification steps.
- **subagent-driven-development** (or executing-plans) — Activates with plan. Dispatches a fresh subagent per task with two-stage review (spec compliance, then code quality), or executes in batches with human checkpoints.
- **test-driven-development** — Activates during implementation. Enforces RED-GREEN-REFACTOR: write failing test, watch it fail, write minimal code, watch it pass, commit. Deletes code written before tests.
- **requesting-code-review** — Activates between tasks. Reviews against plan, reports issues by severity. Critical issues block progress.
- **finishing-a-development-branch** — Activates when tasks complete. Verifies tests, presents options (merge / PR / keep / discard), cleans up worktree.
```

- [ ] **Step 8.4: Commit**

```bash
git add helm/README.md README.md AGENTS.md
git -c commit.gpgsign=false commit -m "docs: README + AGENTS.md for round 2 (clap, scratch, /healthz, single-tag, dev flow)"
```

---

## Task 9: Final quality gate + smoke test

**Files:** none (verification only)

**Interfaces:** full quality gate across all 5 gates.

- [ ] **Step 9.1: Run all 5 gates**

```bash
devenv shell -- bash -c 'cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test'
```

```bash
devenv shell -- bash -c 'helm lint helm --set palworld.image.tag=v1.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000 --set sidecar.image.tag=1.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000'
```

```bash
devenv shell -- bash -c 'shellcheck scripts/*.sh'
```

Expected: clean across all five.

- [ ] **Step 9.2: `helm template` smoke**

```bash
devenv shell -- bash -c 'helm template test ./helm \
  --set palworld.image.tag=v1.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000 \
  --set sidecar.image.tag=1.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000 \
  > /tmp/round-2-rendered.yaml && wc -l /tmp/round-2-rendered.yaml'
```
Expected: complete manifest; no `PLACEHOLDER_REPLACED_BY_TEMPLATE` strings remaining; both containers have `runAsUser: 999`; game service has `ipFamilyPolicy: SingleStack`; metrics service has `ipFamilyPolicy: PreferDualStack`.

- [ ] **Step 9.3: Validation-rules-as-error path**

```bash
devenv shell -- bash -c 'helm template test ./helm 2>&1 | head -3'
```
Expected: error "palworld.image.tag is required ...".

- [ ] **Step 9.4: Commit (no-op unless fixes happened)**

```bash
git status
# if clean:
echo "all gates green; no commit needed"
```

---

## Task 10: Whole-branch review

This is NOT an implementer task — dispatch the requesting-code-review subagent once Tasks 1–9 are clean.

**Files:** no production changes; review report only.

- [ ] **Step 10.1: Capture MERGE_BASE = `git merge-base round-1-master round-2-clap-scratch`**

(or whatever the round-1 anchor commit is — check `git log` for `HEAD~12..HEAD` and identify the commit immediately before this branch started. Round-1 anchor is `7b5e6e8`.)

- [ ] **Step 10.2: Run `scripts/review-package <MERGE_BASE> HEAD` and dispatch the reviewer subagent**

```bash
MERGE_BASE=$(git merge-base round-1-master round-2-clap-scratch 2>/dev/null || git rev-parse 7b5e6e8)
git log --oneline $MERGE_BASE..HEAD
/home/m00n/.cache/opencode/packages/superpowers@git+https:/github.com/obra/superpowers.git/node_modules/superpowers/skills/subagent-driven-development/scripts/review-package $MERGE_BASE HEAD
```

Dispatch a `general` subagent with the review-package path, the design spec, and the plan. Use the code-reviewer-prompt template from the requesting-code-review skill.

- [ ] **Step 10.3: Apply Critical and Important fixes via ONE fix subagent**

Important: ONE fix subagent for the whole findings list (per the subagent-driven-development skill — per-finding fixers rebuild context repeatedly).

- [ ] **Step 10.4: Re-review after fixes**

Re-dispatch the reviewer only if Critical/Important issues were applied. Minor findings stay in the progress ledger.

- [ ] **Step 10.5: Mark Task 10 complete when review approves**

---

## Self-Review

**Spec coverage:**
| Spec § | Topic | Covered by |
|---|---|---|
| §5  | clap for config | Task 1 |
| §5  | Default `metrics_host` to `::` | Task 2 |
| §5  | PALWORLD_API_URL IPv4 literal | Task 1 (clap default) + Task 3 (chart) |
| §6  | `/healthz` endpoint + cached probe | Task 4 |
| §6  | Dockerfile `FROM scratch` + UID 999 | Task 5 |
| §7  | Helm `image.tag` only (no digest split) | Task 6 |
| §7  | UID 999 SCC defaults | Task 7 |
| §7  | IPv4 game service / dualstack metrics | Task 2 + Task 7 |
| §9  | Quality gates | Task 9 |
| §10 | AGENTS.md full dev flow | Task 8 |

**Type consistency:** `Config::load()` (Task 1), `Metrics::palworld_health: Arc<AtomicU8>` (Task 4), `Guard::health_probe: JoinHandle` (Task 4), `image.tag` single field (Task 6), `pod.securityContext` / `containerSecurityContext` namespaces (Task 7). All consistent with the spec's CLI/output shapes.

**Placeholder scan:** no "TBD" / "TODO" / "implement later" in this plan. The Docker validation (Step 5.2) gracefully notes when unavailable.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-29-round-2-clap-scratch-dualstack-implementation.md`. Working in worktree at `~/.worktrees/round-2-clap-scratch/` on branch `round-2-clap-scratch`.

Two execution options:
1. **Subagent-Driven (recommended)** — dispatch a fresh subagent per task with two-stage review.
2. **Inline Execution** — execute tasks in this session using `executing-plans`.

Which approach? (Per AGENTS.md the project default is option 1.)
