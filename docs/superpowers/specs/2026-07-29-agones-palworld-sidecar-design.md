# agones-palworld — design spec

**Status:** design approved 2026-07-29
**Repo:** `/home/m00n/Documents/Projects/Rust/agones-palworld`
**Cluster context:** homelab Kubernetes (`m00nsrv`) via Flux GitOps

## 1. Summary

A Rust sidecar that integrates the Agones game-server SDK with the Palworld
dedicated server's REST API. The sidecar runs as a second container in the same
pod as the game server, bridges Palworld's out-of-process REST API to Agones'
in-process gRPC SDK, and exposes Prometheus metrics via OTel.

The Helm chart at `helm/` ships an Agones `Fleet` (single pod, `Recreate`
strategy), a pass-through `values.yaml` tree, a vendored config-patching script
from the existing `homelab-cluster/.../palworld/app` reference, an auto-generated
admin password, and Prometheus Operator integration.

## 2. Background & constraints

- Palworld is closed-source. The dedicated server (`ghcr.io/pocketpairjp/palserver`)
  is not modifiable; control/observation is via its REST API.
- The REST API requires `RESTAPIEnabled=True` (env var
  `PALWORLD_RESTAPI_ENABLED` per the patch script's `to_env_name` rules) and
  Basic Auth with **empty username** + admin password.
- The server does not read env vars natively; the existing `homelab-cluster/.../palworld/app`
  uses a config-patching script mounted as a ConfigMap that translates
  `PALWORLD_*` env vars into `PalWorldSettings.ini` overrides.
- Agones requires `Ready()` / `Health()` / `Shutdown()` calls from an in-process
  SDK. The Rust SDK (`agones = "1.34"`) connects to `localhost:9358` where the
  Agones sidecar-injected `sdk-server` listens.
- The Fleet runs a single pod at a time (persistent world); replacement pods
  must mount the same PVC.
- Metrics must be scrape-able by Prometheus Operator; the sidecar exposes
  OpenTelemetry-native metrics with a Prometheus exporter adapter.

## 3. Goals

1. Reusable GameServer: when `currentplayernum` drops to 0, the GameServer
   returns to `Ready` for reallocation.
2. Persistent world continuity: player count and player IDs are tracked
   continuously across allocations (no session resets).
3. Out-of-the-box install: `helm install` with sane defaults produces a working
   pod after the operator fills in the game image tag.
4. Operability: Prometheus metrics, structured logs, graceful shutdown that
   saves the world before terminating.

## 4. Non-goals (v1)

- Fleet autoscaling (FleetAutoscaler)
- Backup automation for the SaveGames PVC
- mTLS between sidecar and Palworld REST API
- Web UI / matchmaker
- Multi-cluster allocator wiring
- Image build / push pipeline (CI is documented but not implemented)

## 5. Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ Pod (one per world)                                          │
│  ┌──────────────────────┐  ┌──────────────────────────────┐  │
│  │ palworld (UDP 8211)  │  │ agones-sidecar (this crate)  │  │
│  │  REST API also 8211  │◄─┤  - poll REST :8211           │  │
│  │  RESTAPIEnabled=true │  │  - SDK connect :9358         │  │
│  │                      │  │  - Counter + List ops        │  │
│  └──────────────────────┘  └──────────────────────────────┘  │
│           ▲                          ▲                       │
│           └─── shared PVC for SaveGames ──┘                  │
└─────────────────────────────────────────────────────────────┘
```

**Lifecycle (sidecar's main routine):**

1. **Boot** — load config from env; build `reqwest` REST client targeting
   `PALWORLD_API_URL` (e.g. `http://localhost:8211`); connect to Agones SDK at
   `localhost:9358`; install OTel meter provider + Prometheus scraper on
   `:9090/metrics`; install `tracing` subscriber.
2. **Wait-for-game** — poll `GET /info` with backoff until 200 OK or timeout.
3. **Ready** — call `sdk.ready().await`. GameServer is now `Ready`.
4. **Poll loop** (every `POLL_INTERVAL_SECS`, default 5s):
   - `GET /players` → diff against `WorldState::players`.
   - For each join: `Counter("players").add(1)`, `List("players").append(id)`.
   - For each leave: `Counter("players").add(-1)`, `List("players").delete(id)`.
   - If `currentplayernum > 0` and current Agones state is `Ready` → `sdk.allocate().await`.
   - If `currentplayernum == 0` and current Agones state is `Allocated` → `sdk.set_ready().await`.
   - Update gauges from `GET /metrics` (FPS, frame time, max players, uptime,
     base camps, in-game days, current player count).
5. **Health loop** (every `HEALTH_INTERVAL_SECS`, default 2s) — `sdk.health_check()` ping.
6. **Shutdown** — on SIGTERM: POST `/save`, POST `/announce` with configured
   message, POST `/shutdown` with `waittime=waittimeSeconds`, then
   `sdk.shutdown().await`, then flush OTel + exit. (The `sdk.set_ready()` call
   in step 4 means the GameServer may be `Ready` at shutdown time; the
   `sdk.shutdown()` call still works regardless of current state.)

**Driven by `/players` array, not by `PlayerConnect`/`PlayerDisconnect` RPCs.**
Those Agones SDK functions are alpha; we use the stable Counters/Lists APIs
plus a polled diff.

## 6. Module layout

Single binary crate, library + thin `main.rs`. No workspace.

```
src/
├─ main.rs               # entrypoint, signal handling, task spawn
├─ config.rs             # env-driven Config, validated at boot
├─ palworld.rs           # REST client + endpoint types
├─ agones.rs             # AgonesOps trait + Bridgesdk impl + in-memory mock
├─ state.rs              # WorldState + diff
├─ shutdown.rs           # SIGTERM orchestration
├─ observability.rs      # OTel meter, Prometheus scraper, tracing init
├─ error.rs              # AppError + From impls
└─ (tests in #[cfg(test)] modules + tests/)
```

**Module interfaces:**

- `Config::from_env() -> Result<Config>` — typed, validated once at boot.
- `palworld::Client` — methods `info`, `players`, `metrics`, `settings`,
  `save`, `announce`, `shutdown`, `stop`, `kick`, `ban`, `unban`. All async,
  return `Result<_, AppError>`. Holds a `reqwest::Client` and pre-computed
  Basic-auth header.
- `agones::AgonesOps` — trait defined for testability; production impl
  `Bridge` wraps `agones::Sdk`. Methods: `connect`, `ready`, `allocate`,
  `set_ready`, `health_ping`, `shutdown`, `current_state`, `counter_add`,
  `counter_set`, `list_append`, `list_delete`, `list_clear`.
- `state::WorldState` — `version: String`, `worldguid: String`,
  `players: BTreeSet<PlayerId>`; `observe(&[Player]) -> PlayerDiff { joined, left }`.
- `shutdown::run(client, bridge) -> Result<()>` — orchestrates the
  save → announce → shutdown → SDK shutdown sequence with a hard deadline.

## 7. REST client (`palworld.rs`)

- **Auth:** Basic auth with empty username + `AdminPassword`. The header
  `Authorization: Basic base64(":<password>")` is computed once at startup and
  held in the struct. The password is read from env (`PALWORLD_ADMIN_PASSWORD`)
  and never logged (debug formatter redacts).
- **Timeouts:** `reqwest::Client` configured with a per-request timeout
  (default 5s) and connect timeout (default 2s).
- **Retries:** No automatic retries on write operations (`save`, `shutdown`,
  `kick`, `ban`, `unban`). Read operations (`info`, `players`, `metrics`,
  `settings`) get a small retry budget (3 attempts, exponential backoff)
  within the poll cycle.
- **REST base URL:** the sidecar reads `PALWORLD_API_URL` from env (e.g.
  `http://localhost:8211`). The chart sets this to `http://localhost:{{ .Values.palworld.restPort }}`
  in the sidecar container env. Override via `palworld.restPort` in
  `values.yaml` (default 8211) or via the `sidecar.env.PALWORLD_API_URL` escape hatch.
  No port discovery — the port must be configured, since the sidecar has
  no way to query `/settings` without already knowing the URL.
- **Endpoints used:** `GET /info`, `GET /players`, `GET /metrics`, `POST /save`,
  `POST /announce`, `POST /shutdown`. `/settings` is implemented for parity
  but not called by v1. The other endpoints
  (`/kick`, `/ban`, `/unban`, `/stop`) are implemented for completeness but
  not called by v1 logic.

## 8. State management (`state.rs`)

```rust
pub struct WorldState {
    pub version: String,
    pub worldguid: String,
    pub players: BTreeSet<PlayerId>,
}

pub struct PlayerDiff {
    pub joined: Vec<PlayerId>,
    pub left: Vec<PlayerId>,
}

impl WorldState {
    pub fn observe(&mut self, players: &[Player]) -> PlayerDiff {
        let current: BTreeSet<_> = players.iter().map(|p| p.player_id.clone()).collect();
        let joined = current.difference(&self.players).cloned().collect();
        let left = self.players.difference(&current).cloned().collect();
        self.players = current;
        PlayerDiff { joined, left }
    }
}
```

**Source of truth:** `/players` array. A player only counts as "joined" on the
first sighting and "left" on the first disappearance.

**Reconciliation:** `/metrics.currentplayernum` is exposed as a separate gauge
(`palworld.players.current`) for sanity checking. If it disagrees with `players.len()`,
we log a warning and trust the next poll.

## 9. Observability

**Crate stack:**

- `opentelemetry = "0.27"`, `opentelemetry_sdk = "0.27"` (runtime Tokio)
- `opentelemetry-otlp = "0.27"` (gRPC exporter)
- `opentelemetry-prometheus = "0.16"` (scraper adapter)
- `prometheus = "0.13"`
- `tracing = "0.1"`, `tracing-subscriber = "0.3"` (env-filter + json)
- `tracing-opentelemetry = "0.28"` (bridge)

**Pipeline:**

```
tracing spans  ──┐
                  ├─► tracing-opentelemetry ──► OTLP exporter (optional) ──► Collector
stdout JSON     ──┘
metrics instrs ──► MeterProvider ─┬─► PeriodicExporter (optional OTLP)
                                   └─► opentelemetry-prometheus scraper ──► :9090/metrics
```

**Metric inventory** (OTel names → Prometheus scrape names):

| OTel | Prometheus | Type | Source |
|---|---|---|---|
| `palworld.sidecar.poll_cycles` | `palworld_sidecar_poll_cycles_total` | counter | poll loop |
| `palworld.sidecar.poll_errors` | `palworld_sidecar_poll_errors_total` | counter | transport errors |
| `palworld.sidecar.player_joins` | `palworld_sidecar_player_joins_total` | counter | diff |
| `palworld.sidecar.player_leaves` | `palworld_sidecar_player_leaves_total` | counter | diff |
| `palworld.sidecar.agones_ops` | `palworld_sidecar_agones_ops_total` | counter | SDK calls |
| `palworld.sidecar.ready_state` | `palworld_sidecar_ready_state` | gauge | Agones state |
| `palworld.sidecar.last_successful_poll_unixtime` | `palworld_sidecar_last_successful_poll_unixtime` | gauge | wall clock |
| `palworld.sidecar.build_info` | `palworld_sidecar_build_info` | gauge (always 1) | constant |
| `palworld.sidecar.uptime_seconds` | `palworld_sidecar_uptime_seconds` | gauge | monotonic |
| `palworld.server.fps` | `palworld_server_fps` | gauge | `/metrics` |
| `palworld.server.frame_time_ms` | `palworld_server_frame_time_ms` | gauge | `/metrics` |
| `palworld.server.uptime_seconds` | `palworld_server_uptime_seconds` | gauge | `/metrics` |
| `palworld.players.current` | `palworld_players_current` | gauge | `/metrics` |
| `palworld.players.max` | `palworld_players_max` | gauge | `/metrics` |
| `palworld.players.connected` | `palworld_players_connected` | gauge | `/players` |
| `palworld.world.base_camp_count` | `palworld_world_base_camp_count` | gauge | `/metrics` |
| `palworld.world.in_game_days` | `palworld_world_in_game_days` | gauge | `/metrics` |

**Resource attributes** on the OTel `Resource`:
- `service.name = "agones-palworld"`
- `service.namespace = "palworld"`
- `service.version = env!("CARGO_PKG_VERSION")`
- `k8s.pod.name = $POD_NAME` (downward API)
- `k8s.namespace.name = $POD_NAMESPACE`
- `k8s.container.name = "agones-sidecar"`

**Tracing spans:**
- `poll_cycle` wraps each tick
- `agones_op` (with `op` + `result` fields) wraps each SDK call
- `shutdown_sequence` wraps the SIGTERM flow
- `palworld_request` auto-injected by `#[instrument]` on each REST method,
  logs HTTP status code, never the password

**Env vars:**

| Var | Default | Behavior |
|---|---|---|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | *(unset)* | If set, OTLP exporter installed; if unset, only Prometheus scrape works |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | `grpc` | `grpc` / `http/protobuf` / `http/json` |
| `METRICS_PORT` | `9090` | Prome scraper port |
| `METRICS_HOST` | `0.0.0.0` | Prome scraper host |
| `DISABLE_PROMETHEUS` | `false` | Skip scraper install |
| `LOG_FORMAT` | `pretty` | `pretty` or `json` |
| `RUST_LOG` | `info,agones=warn,hyper=warn,h2=warn` | EnvFilter |

**Shutdown hook:** `opentelemetry::global::shutdown_tracer_provider()` +
`meter_provider.shutdown()` called before exit so the last batch of metrics is
flushed.

## 10. Helm chart

**Files:**

```
helm/
├─ Chart.yaml
├─ values.yaml
├─ README.md
├─ files/
│  └─ patch-palworld-settings.sh
└─ templates/
   ├─ _helpers.tpl
   ├─ fleet.yaml
   ├─ service.yaml                  # game UDP Service
   ├─ metrics-service.yaml          # sidecar :9090 metrics
   ├─ servicemonitor.yaml           # Prometheus Operator
   ├─ pvc.yaml
   ├─ secret.yaml
   ├─ configmap.yaml
   └─ NOTES.txt
```

**Values philosophy:** every key is one of:
- **(a)** a chart opinion we hold firmly (sensible default),
- **(b)** an escape hatch to override an opinion,
- **(c)** a full pass-through for things we have no claim on (operator-owned maps/lists).

**`values.yaml` skeleton:**

```yaml
fleet:
  replicas: 1
  strategy: { type: Recreate }
  scheduling: Packed
  template:
    metadata: { annotations: { agones.dev/sdk-server: agones-sidecar } }
    spec:
      ports: [ { name: game, portPolicy: NodePort, containerPort: 8211, protocol: UDP } ]
      health: { periodSeconds: 2, failureThreshold: 3 }
      sdkServer: { grpcPort: 9358, httpPort: 9359 }
      # Chart-managed container defaults live here. Operator overrides
      # any field by editing this list. The palworld container's image
      # tag is interpolated from .Values.palworld.image.tag at render time.
      # The sidecar container's image tag is interpolated from
      # .Values.sidecar.image.tag (and optional digest).
      containers:
        - name: palworld
          image: ghcr.io/pocketpairjp/palserver
          imagePullPolicy: IfNotPresent
          command: ["/bin/bash", "/scripts/patch-palworld-settings.sh"]
          args: ["-port=8211", "-useperfthreads", "-NoAsyncLoadingThread", "-UseMultithreadForDS"]
          env:
            - name: PALWORLD_RESTAPI_ENABLED
              value: "True"
          envFrom:
            - secretRef: { name: RELEASENAME-admin }
          ports:
            - { name: game, containerPort: 8211, protocol: UDP }
          volumeMounts:
            - { name: scripts, mountPath: /scripts }
            - { name: savegames, mountPath: /pal/Package/Pal/Saved }
        - name: agones-sidecar
          image: ghcr.io/m00nwtchr/agones-palworld
          imagePullPolicy: IfNotPresent
          env:
            - name: PALWORLD_ADMIN_PASSWORD
              valueFrom: { secretKeyRef: { name: RELEASENAME-admin, key: palworld_admin_password } }
            - name: PALWORLD_API_URL
              value: "http://localhost:8211"
            - name: POD_NAME
              valueFrom: { fieldRef: { fieldPath: metadata.name } }
            - name: POD_NAMESPACE
              valueFrom: { fieldRef: { fieldPath: metadata.namespace } }
          ports:
            - { name: metrics, containerPort: 9090, protocol: TCP }
      volumes:
        - name: scripts
          configMap: { name: RELEASENAME-config }
        - name: savegames
          persistentVolumeClaim: { claimName: RELEASENAME-savegames }
```

**Image tag interpolation** (handled by the Helm template, not values):

```yaml
spec:
  containers:
    - name: palworld
      image: "{{ .Values.palworld.image.repository }}:{{ .Values.palworld.image.tag }}"
    - name: agones-sidecar
      image: "{{ .Values.sidecar.image.repository }}:{{ .Values.sidecar.image.tag }}{{ if .Values.sidecar.image.digest }}@{{ .Values.sidecar.image.digest }}{{ end }}"
```

The chart name is substituted by the template helper (`RELEASENAME` →
`{{ include "agones-palworld.fullname" . }}`). Operator edits values.yaml
under `fleet.template.spec.containers[]` to override any field (e.g.,
resources, securityContext, extra volumeMounts, additional env vars).
For adding NEW env vars, use `palworld.env: {}` and `palworld.envFrom: []`
escape hatches (see below) — the template merges them into the chart-managed
list at render time.

palworld:
  image:
    repository: ghcr.io/pocketpairjp/palserver
    tag: ""               # MUST be set at install time
    pullPolicy: IfNotPresent
  restPort: 8211
  gamePort: 8211
  env: { }                # PALWORLD_* overrides
  envFrom: [ ]            # extra EnvFromSources

sidecar:
  image:
    repository: ghcr.io/m00nwtchr/agones-palworld
    tag: "{{ .Chart.AppVersion }}"
    digest: ""            # CI pins sha256:...
    pullPolicy: IfNotPresent
  pollIntervalSeconds: 5
  healthIntervalSeconds: 2
  shutdown:
    saveTimeoutSeconds: 30
    waittimeSeconds: 30
    announceMessage: "Server shutting down"
  env: { }
  envFrom: [ ]

otel:
  enabled: true
  endpoint: ""
  protocol: "grpc"
  serviceName: "agones-palworld"
  serviceNamespace: "palworld"
  serviceVersion: ""

metrics:
  service:
    enabled: true
    type: ClusterIP
    port: 9090
    annotations: { }
    labels: { }
  serviceMonitor:
    enabled: true
    interval: 30s
    scrapeTimeout: 10s
    path: /metrics
    scheme: HTTP
    labels: { }
    namespaceSelector: ""
    honorLabels: false

pvc:
  enabled: true
  existingClaim: ""
  size: 50Gi
  storageClass: ""
  accessModes: [ReadWriteOnce]
  mountPath: /pal/Package/Pal/Saved

service:
  type: Headless            # LoadBalancer | ClusterIP | Headless
  port: 8211
  protocol: UDP
  annotations: { }
  labels: { }

secret:
  enabled: true
  existingSecret: ""
  passwordKey: "palworld_admin_password"
  autoGenerate: true
  length: 32
```

**Pre-flight validation (in `_helpers.tpl`):**

- `palworld.image.tag` empty → fail
- `sidecar.image.tag` AND `sidecar.image.digest` both empty → fail
- `secret.enabled=false` AND no `valueFrom.secretKeyRef` on the sidecar container → fail
- `metrics.serviceMonitor.enabled=true` AND `metrics.service.enabled=false` → fail
- `palworld.env` contains keys not prefixed `PALWORLD_` → fail (with the offending key in the error)
- `palworld.env.PALWORLD_RESTAPI_ENABLED == "False"` → fail

**Fleet template (key fragments):**

```yaml
apiVersion: agones.dev/v1
kind: Fleet
metadata:
  name: {{ include "agones-palworld.fullname" . }}
  labels:
    {{- include "agones-palworld.labels" . | nindent 4 }}
spec:
  replicas: {{ .Values.fleet.replicas }}
  strategy: {{- toYaml .Values.fleet.strategy | nindent 4 }}
  scheduling: {{ .Values.fleet.scheduling }}
  template:
    metadata: {{- toYaml .Values.fleet.template.metadata | nindent 6 }}
    spec: {{- toYaml .Values.fleet.template.spec | nindent 6 }}
```

**Chart-managed defaults inside `values.fleet.template.spec`** (the operator may
override any of these):

```yaml
containers:
  - name: palworld
    image: {{ .Values.palworld.image.repository }}:{{ .Values.palworld.image.tag }}
    imagePullPolicy: {{ .Values.palworld.image.pullPolicy }}
    command: ["/bin/bash", "/scripts/patch-palworld-settings.sh"]
    args: ["-port=8211", "-useperfthreads", "-NoAsyncLoadingThread", "-UseMultithreadForDS"]
    env:
      - name: PALWORLD_RESTAPI_ENABLED
        value: "True"
      {{- with .Values.palworld.env }}
      {{- range $k, $v }}
      - name: {{ $k | quote }}
        value: {{ $v | quote }}
      {{- end }}
      {{- end }}
    envFrom:
      - secretRef: { name: {{ include "agones-palworld.fullname" . }}-admin }
      {{- with .Values.palworld.envFrom }}
      {{- toYaml . | nindent 6 }}
      {{- end }}
    volumeMounts:
      - { name: scripts, mountPath: /scripts }
      - { name: savegames, mountPath: {{ .Values.pvc.mountPath }} }

  - name: agones-sidecar
    image: "{{ .Values.sidecar.image.repository }}:{{ .Values.sidecar.image.tag }}{{ if .Values.sidecar.image.digest }}@{{ .Values.sidecar.image.digest }}{{ end }}"
    imagePullPolicy: {{ .Values.sidecar.image.pullPolicy }}
    env:
      - name: PALWORLD_ADMIN_PASSWORD
        valueFrom:
          secretKeyRef:
            name: {{ include "agones-palworld.fullname" . }}-admin
            key: palworld_admin_password
      - name: PALWORLD_API_URL
        value: "http://localhost:{{ .Values.palworld.restPort }}"
      - name: POD_NAME
        valueFrom: { fieldRef: { fieldPath: metadata.name } }
      - name: POD_NAMESPACE
        valueFrom: { fieldRef: { fieldPath: metadata.namespace } }
      {{- with .Values.sidecar.env }}
      {{- range $k, $v }}
      - name: {{ $k | quote }}
        value: {{ $v | quote }}
      {{- end }}
      {{- end }}
    envFrom:
      {{- with .Values.sidecar.envFrom }}
      {{- toYaml . | nindent 6 }}
      {{- end }}
    ports:
      - { name: metrics, containerPort: 9090, protocol: TCP }

volumes:
  - name: scripts
    configMap: { name: {{ include "agones-palworld.fullname" . }}-config }
  - name: savegames
    persistentVolumeClaim:
      claimName: {{ .Values.pvc.existingClaim | default (printf "%s-savegames" (include "agones-palworld.fullname" .)) }}
```

**Auto-generated Secret** (`templates/secret.yaml`):

```yaml
apiVersion: v1
kind: Secret
metadata:
  name: {{ include "agones-palworld.fullname" . }}-admin
  labels:
    {{- include "agones-palworld.labels" . | nindent 4 }}
stringData:
  palworld_admin_password: |-
    {{- $existing := (lookup "v1" "Secret" .Release.Namespace (printf "%s-admin" (include "agones-palworld.fullname" .))) }}
    {{- if and $existing $existing.data.palworld_admin_password }}
    {{- index $existing.data "palworld_admin_password" | b64dec }}
    {{- else }}
    {{- randAlphaNum 32 }}
    {{- end }}
```

**ConfigMap** (`templates/configmap.yaml`) holds the `patch-palworld-settings.sh`
script, vendored from `homelab-cluster/.../palworld/app/resources/patch-palworld-settings.sh`.

**Metrics Service** (`templates/metrics-service.yaml`): ClusterIP with selector
matching the Fleet pods (via `app.kubernetes.io/instance={{ .Release.Name }}`).
The Fleet pods must carry this label — the Fleet template includes
`agones-palworld.labels` to set it.

**ServiceMonitor** (`templates/servicemonitor.yaml`):
`monitoring.coreos.com/v1`, selects the metrics Service, endpoint port
`http-metrics` path `/metrics`, interval 30s, timeout 10s.

**PVC** (`templates/pvc.yaml`): conditional on `pvc.enabled` AND
`pvc.existingClaim == ""`. RWO, default 50Gi.

**Game Service** (`templates/service.yaml`): configurable type (Headless /
ClusterIP / LoadBalancer), port 8211, protocol UDP, selector matching Fleet
pods.

## 11. Dockerfile + build script

**Multi-stage Dockerfile:**

```dockerfile
FROM rust:1.82-bookworm AS builder
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
```

**`scripts/build-image.sh`:** POSIX shell, idempotent, reads `IMAGE` and `TAG`
from env (defaults from `Cargo.toml`), runs `docker buildx build --load` with
`org.opencontainers.image.version` label. Optional `--push` flag.

## 12. Error handling

**`AppError` categories:**

- `AppError::Config(String)` — boot-time, fatal
- `AppError::Agones(agones::SdkError)` — recoverable, retry
- `AppError::PalworldHttp(StatusCode, String)` — per-endpoint retry policy
- `AppError::PalworldTimeout` — recoverable
- `AppError::Signal` — drives graceful exit

**Resilience rules:**

- Polling loops never panic; on REST error, log + increment counter + skip this tick.
- Agones SDK is internally retrying; long disconnection → Agones marks pod unhealthy → reschedule.
- Shutdown sequence has a hard deadline (`shutdown.saveTimeoutSeconds`); if save fails, log + continue (don't block termination).
- `RUST_BACKTRACE=1` only on debug; release defaults to `0`.

## 13. Testing strategy

| Layer | Test |
|---|---|
| `state::WorldState` | Pure unit tests for `observe()` diff on synthetic player lists |
| `palworld::Client` | `wiremock` against each endpoint; asserts Basic auth header, JSON parsing, retry behavior, timeout |
| `agones::Bridge` | `AgonesOps` trait with in-memory mock; tests Allocate-on-first-non-zero, set_ready-on-zero |
| `shutdown` | Mocked client + bridge; asserts save → announce → shutdown → sdk.shutdown ordering |
| Observability | Construct provider + Prometheus exporter, emit counter, scrape `/metrics`, assert text format |
| Helm chart | `helm template` golden output; `helm unittest` for validation rules |

## 14. CI integration (documented, not implemented)

When the chart version is released, CI:
1. Builds the sidecar image with `tag = Chart.version`
2. Pushes to `ghcr.io/m00nwtchr/agones-palworld`
3. Resolves the image digest and updates a `values-pinned.yaml` consumed by
   Flux's `HelmRelease.spec.valuesFrom` so the deployed manifest has the
   immutable digest.

The chart stays pure; CI owns the digest pinning.

## 15. Cargo dependency list

```toml
[dependencies]
agones = "1.34"
tokio = { version = "1.32", features = ["macros", "rt-multi-thread", "sync", "signal", "time"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
url = "2"
base64 = "0.22"
opentelemetry = "0.27"
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-otlp = "0.27"
opentelemetry-prometheus = "0.16"
prometheus = "0.13"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-opentelemetry = "0.28"

[dev-dependencies]
wiremock = "0.6"
rstest = "0.23"
pretty_assertions = "1"
```

## 16. Reference implementations

- Existing config-patching script: `homelab-cluster/kubernetes/apps/games/palworld/app/resources/patch-palworld-settings.sh`
- Existing HelmRelease: `homelab-cluster/kubernetes/apps/games/palworld/app/helmrelease.yaml`
- Agones Rust SDK doc: https://agones.dev/site/docs/guides/client-sdks/rust/
- Palworld REST API doc: https://docs.palworldgame.com/category/rest-api
