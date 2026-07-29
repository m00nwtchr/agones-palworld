# agones-palworld — round 2 design spec

**Status:** design approved 2026-07-29
**Repo:** `/home/m00n/Documents/Projects/Rust/agones-palworld`
**Cluster context:** homelab Kubernetes (`m00nsrv`) via Flux GitOps
**Supersedes:** N/A (additive to v0.1.0)

## 1. Summary

Five adjustments to the v0.1.0 implementation, plus two follow-on corrections from the brainstorming session:

1. **clap for config** — replace `std::env::var` with clap derive API; CLI flags accepted alongside env vars.
2. **`FROM scratch` runtime** — Dockerfile goes from distroless/cc-debian12 to `scratch`, requiring musl + static linking.
3. **`/healthz` endpoint on the sidecar** — Kubernetes liveness/readiness probe target; checks both the sidecar itself and the Palworld REST API (IPv4 only).
4. **Dualstack default bind** — metrics HTTP listener binds `[::]` (IPv6 wildcard, accepts IPv4 via Linux's `IPV6_V6ONLY=0`); metrics service uses `ipFamilyPolicy: PreferDualStack`. **Game service stays IPv4-only** because the Palworld server is IPv4 only.
5. **Single image tag (no separate digest field)** — `image.tag` carries the `version@sha256:digest` string; the `digest` field is removed.

Additional clarifications:
6. **PUID/PGID = 999** — both containers run as UID 999 (Palworld image's contract). Pod `fsGroup: 999` matches.
7. **Game service is IPv4-only** — Palworld server is single-stack IPv4; the Service serving players must mirror that.

## 2. Background & constraints

- Round 1 shipped v0.1.0 (commit `7b5e6e8` on `master`). This round is additive — no existing behavior is regressed.
- Agones, Palworld REST API, and the patching-script constraints are unchanged from round 1.
- The `palworld` server image (`ghcr.io/pocketpairjp/palserver`) runs as UID 999 by default. Shared PVC mounts require the sidecar to use the same UID so file ownership is consistent.

## 3. Goals

1. **Configuration via clap** — operators can pass `--api-url http://...` on the CLI while the same env var continues to work for containers.
2. **`FROM scratch` image** — minimal CVE surface; statically-linked musl binary.
3. **Kubernetes-native healthcheck** — `/healthz` endpoint that reflects both the sidecar process and the Palworld API.
4. **Dualstack metrics, IPv4 game** — metrics endpoint reachable via v4 and v6; game service stays v4 to match the game server.
5. **Single image tag** — `image.repository` defaults in the chart; `image.tag` is the moving part.
6. **UID 999 everywhere** — Pod, both containers, sidecar user in `FROM scratch`.

## 4. Non-goals (round 2)

- Fleet autoscaler, backup automation, mTLS, hot-reload of config, CI pipeline (still out of scope from round 1).
- Changing the Agones state-transition logic or the polling loop.
- Adding new endpoints to the Palworld REST client.

## 5. Architecture (changes only)

```
+---------------------+
| agones-sidecar      |
|  metrics HTTP       |
|  bind [::]:9090     |   <- dualstack (accepts v4 via IPV6_V6ONLY=0)
|  /metrics           |
|  /healthz           |   <- new
+---------------------+
         |
         |  http://127.0.0.1:8211  (IPv4 only)
         v
+---------------------+
| palworld container  |
|  runs as UID 999    |
|  PUID/PGID env vars |
+---------------------+
```

Outside the pod, the **game UDP Service** is `ipFamilyPolicy: SingleStack, ipFamilies: [IPv4]`; the **metrics Service** is `ipFamilyPolicy: PreferDualStack, ipFamilies: [IPv6, IPv4]`.

## 6. Module-level changes (Rust)

### `src/config.rs` — clap derive
- Replace `Config::from_env` with `Config::load()` that calls `<Self as clap::Parser>::parse()`.
- Each field gets `#[arg(long, env = "...", default_value = "...")]`. CLI flag > env > default.
- `api_url` defaults to `"http://127.0.0.1:8211"` (IPv4 literal, per user's note that the game server is IPv4 only).
- `metrics_host` defaults to `"::"` (was `"0.0.0.0"`).
- Tests rewrite to use `Config::parse_from(&["agones-palworld", "--api-url", ..., "--admin-password", ...])` — no env mutation, no `Mutex` needed.

### `src/observability.rs` — `/healthz`
- Add an `AtomicU8` (`palworld_health`) to `Metrics`. Values: 0 unknown, 1 healthy, 2 unhealthy.
- Spawn a background tokio task in `install()` that polls `client.info()` every 5 s and writes the atomic.
- HTTP handler branches on path: `/metrics` → Prometheus text; `/healthz` → 200 if atomic is healthy, 503 otherwise. Other paths → 404.
- `handle()` returns a JSON body for `/healthz` so operators can curl it for debugging.

### `Dockerfile` — `FROM scratch` with musl
- Builder: `rust:bookworm`, install `musl-tools`, add `x86_64-unknown-linux-musl` target via `rustup`.
- `RUSTFLAGS="-C target-feature=+crt-static"`, build with `--target x86_64-unknown-linux-musl`.
- Runtime: `FROM scratch`. Copy: `/agones-palworld`, `/etc/passwd` (with UID 999 entry), `/etc/ssl/certs/ca-certificates.crt`.
- `USER 999:999` (matches Palworld UID).
- `EXPOSE 9090`, `ENTRYPOINT ["/agones-palworld"]`.

### `Cargo.toml` — clap dep
```toml
clap = { version = "4", features = ["derive", "env"] }
```
Nothing else changes.

## 7. Helm chart changes

### `helm/values.yaml`
```yaml
palworld:
  image:
    repository: ghcr.io/pocketpairjp/palserver
    tag: ""                  # operator-supplied; format: "v1.0.0@sha256:..."
    pullPolicy: IfNotPresent
  env: { }
  envFrom: [ ]

sidecar:
  image:
    repository: ghcr.io/m00nwtchr/agones-palworld
    tag: ""                  # defaulted in chart to .Chart.AppVersion via `default` template helper
    pullPolicy: IfNotPresent
  env: { }
  envFrom: [ ]

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

service:
  type: Headless
  port: 8211
  protocol: UDP
  ipFamilyPolicy: SingleStack       # Game server is IPv4-only
  ipFamilies: [IPv4]
  annotations: { }
  labels: { }

metrics:
  service:
    enabled: true
    type: ClusterIP
    ipFamilyPolicy: PreferDualStack # metrics listener binds [::]
    ipFamilies: [IPv6, IPv4]
    port: 9090
```

### `helm/templates/_helpers.tpl`
- Remove the `sidecar.image.digest` check.
- `palworld.image.tag` empty → fail.
- `sidecar.image.tag` empty (after template `default .Chart.AppVersion`) → fail.

### `helm/templates/service.yaml` (game UDP)
```yaml
spec:
  type: {{ .Values.service.type }}
  ipFamilyPolicy: {{ .Values.service.ipFamilyPolicy }}
  ipFamilies: {{- toYaml .Values.service.ipFamilies | nindent 4 }}
```

### `helm/templates/metrics-service.yaml`
Same kind of `ipFamilyPolicy` / `ipFamilies` blocks, defaulted to `PreferDualStack` / `[IPv6, IPv4]`.

### `helm/templates/fleet.yaml`
- Both containers' `securityContext` block: `{{- toYaml .Values.containerSecurityContext | nindent 8 }}` (so the operator override in `values.yaml` flows through).
- Pod-level `securityContext` from `.Values.pod.securityContext`.
- Container `image`: `image: "{{ .Values.X.image.repository }}:{{- default .Chart.AppVersion .Values.X.image.tag -}}"` — the `default` falls back to chart version for sidecar; for palworld the empty tag fails the validation rule.

### `helm/README.md` + root `README.md`
Replace `palworld.image.tag="vX.X.X@sha256:..."` examples. Add a note: "Pin the sidecar image tag with `@sha256:<digest>` via CI to make deployments immutable."

## 8. Observability (changes only)

- New OTel instrument: `palworld.sidecar.palworld_reachable` (gauge 0/1) — fed by the cached background probe.
- Existing metric naming unchanged.
- `/healthz` response body (JSON):
  ```json
  {"sidecar":"ok","palworld":"ok"}
  ```
  Returns 503 + same body shape with `"palworld":"down"` when the cache says unhealthy.

## 9. Quality gates

Unchanged from round 1:
- `cargo fmt --all --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `helm lint helm --set palworld.image.tag="v0.0.0@sha256:0000000000000000000000000000000000000000000000000000000000000000"`
- `shellcheck scripts/*.sh`

New gate in round 2:
- `docker buildx build --load --tag agones-palworld:test . 2>&1 | tail -5` (if docker available; otherwise note skipped) — confirms the musl build still produces a binary that copies into `scratch`.

## 10. Implementation tasks (high level)

1. Add clap dep; refactor `Config::from_env` → `Config::load` with derive; rewrite tests using `parse_from`.
2. Add `/healthz` handler + `AtomicU8` + background probe task in `observability.rs`.
3. Switch `metrics_host` default to `::` and update any docs/tests.
4. Default `PALWORLD_API_URL` to `http://127.0.0.1:8211` (IPv4 literal).
5. Update Dockerfile for musl + `FROM scratch` + UID 999 + ca-certs.
6. Collapse `sidecar.image.digest` field; update fleet template assembly; update _helpers.tpl validation.
7. Add `pod.securityContext` + `containerSecurityContext` defaults with UID 999.
8. Set `ipFamilyPolicy` / `ipFamilies` on game UDP service (IPv4) and metrics service (dualstack).
9. Update `helm/README.md` and root `README.md` for the new image-tag convention and the dualstack metric service.
10. Append the full superpowers dev flow to `AGENTS.md`.
11. Quality gate + final whole-branch review (subagent).

## 11. Out of scope

- Fleet autoscaler, backup automation, mTLS, CI pipeline, hot-reload of config.

## 12. References

- Round 1 design: `docs/superpowers/specs/2026-07-29-agones-palworld-sidecar-design.md`
- Round 1 plan: `docs/superpowers/plans/2026-07-29-agones-palworld-sidecar-implementation.md`
- Existing homelab-cluster release: `homelab-cluster/kubernetes/apps/games/palworld/app/helmrelease.yaml` (sets `runAsUser: 999`, `runAsGroup: 999`, `fsGroup: 999`).
- Palworld server image: `ghcr.io/pocketpairjp/palserver`.
- Agones Rust SDK: `agones = "1.59"` resolved in round 1.
