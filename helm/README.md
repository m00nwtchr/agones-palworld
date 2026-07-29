# agones-palworld Helm chart

Provisions an Agones Fleet running a Palworld dedicated server with a Rust
sidecar that bridges the Palworld REST API to the Agones SDK.

## Install

```bash
helm install palworld ./helm \
  --namespace games --create-namespace \
  --set palworld.image.tag=v1.0.1.100619@sha256:0d293cafd503a91a6d11d71f7bf770ee0c3c5ecf37db988349b2c1758f4e9358 \
  --set sidecar.image.tag=0.1.0@sha256:<digest-pinned-by-ci>
```

> **Pin the sidecar image tag with `X.Y.Z@sha256:<digest>` via CI for immutability.**
> The chart supports both bare `X.Y.Z` (mutable, defaults to chart `appVersion`) and the
> digest-pinned form. CI should resolve the digest at build time and substitute it into
> the manifest before deploy.

## UID/GID 999

The chart pins both the Palworld and sidecar containers to `runAsUser: 999` /
`runAsGroup: 999`. The `scratch`-based sidecar image bundles `/etc/passwd` with the
matching entry (`palworld:x:999:999:...`), so the in-container `nobody` UID the kernel
would otherwise pick aligns with the Pod's security context. This lets the
`readOnlyRootFilesystem: true` Pod still bind-mount the savegame PVC with correct
ownership without granting any extra capabilities.

## Read the admin password

```bash
kubectl get secret -n games palworld-admin -o jsonpath='{.data.palworld_admin_password}' | base64 -d
```

## Metrics

- Service: `kubectl get svc palworld-metrics`
- ServiceMonitor: requires Prometheus Operator (CRDs `monitoring.coreos.com/v1`).

## Override anything

The `values.yaml` is opinionated with escape hatches. Common overrides:

```yaml
palworld:
  env:
    PALWORLD_SERVER_NAME: "My Server"
  envFrom:
    - secretRef: { name: rcon-credentials }

sidecar:
  env:
    LOG_FORMAT: "json"

metrics:
  serviceMonitor:
    enabled: false
```

For Fleet-level changes, edit `fleet.template.spec.containers[]` directly — it is
fully pass-through.
