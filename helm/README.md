# agones-palworld Helm chart

Provisions an Agones Fleet running a Palworld dedicated server with a Rust
sidecar that bridges the Palworld REST API to the Agones SDK.

## Install

```bash
helm install palworld ./helm \
  --namespace games --create-namespace \
  --set palworld.image.tag=v1.0.1.100619@sha256:0d293cafd503a91a6d11d71f7bf770ee0c3c5ecf37db988349b2c1758f4e9358 \
  --set sidecar.image.digest=sha256:<digest-pinned-by-ci>
```

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
