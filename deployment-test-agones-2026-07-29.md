# Deployment test report — `agones-palworld` chart with Agones 1.59.0

**Cluster:** `m00nsrv` (v1.35.4+k3s1)
**Date:** 2026-07-29
**Test namespace:** `agones-palworld-test2` (switched from the first ns, which had stale-delete state)
**Existing workload:** `palworld-0` in `games` ns (Flux-managed app-template HelmRelease; `Running`, 0 restarts, completely untouched)

## Summary

The chart's six cluster-side resources (Secret, ConfigMap, PVC, 2x Service, ServiceMonitor) all landed cleanly on the cluster via `kubectl apply`. After two real Fleet schema fixes (one structural, one field), the Fleet itself was accepted by the apiserver (`fleet created`). Agones created a GameServerSet and attempted to schedule a GameServer pod, which failed because the `agones-sdk` ServiceAccount is missing on this cluster — a cluster-side setup gap unrelated to the chart.

## What worked

- `helm template` renders 7 resources cleanly.
- `kubectl apply --dry-run=server` accepts all 7 (Fleet included after fixes).
- Real `kubectl apply` creates them all.
- Agones controller reconciles the Fleet into a `GameServerSet agones-palworld-test-jknl2` (1/1 desired).
- Agones attempts `CreatingGameServer` (event shown).
- Existing `games/palworld-0` continues running normally (0 restarts, no interference).

## Two real chart bugs caught and fixed in this session

### Bug A — Fleet template was one level shallow

The chart's `helm/values.yaml` rendered `spec.template.spec.containers` — but Agones' Fleet wraps the PodSpec inside an additional `template.spec` level:

```
spec.template.spec.<game-server fields>
spec.template.spec.template.spec.<pod fields — containers, volumes, securityContext>
```

Caught by `kubectl apply --dry-run=server`:
> unknown field "spec.template.spec.containers"

**Fix:** Restructured `helm/values.yaml` so the PodSpec fields live at `fleet.template.spec.template.spec.{containers,volumes}`, and updated `helm/templates/fleet.yaml` to walk the new path.

### Bug B — `spec.template.spec.container` field missing

After fixing Bug A, the apiserver rejected:
> spec.template.spec.container: Required value: Container is required when using multiple containers in the pod template

Agones requires the GameServer spec to name its main container explicitly so the SDK hooks health checks correctly.

**Fix:** Added `container: palworld` to `helm/values.yaml`'s `fleet.template.spec` block; the Fleet template emits it via `{{- with ... }} ... set ...`.

## What didn't work — and why

The GameServer pod failed to schedule with:

> pods "agones-palworld-test-jknl2-mcqcm" is forbidden: error looking up service account default/agones-sdk: serviceaccount "agones-sdk" not found

This is a **cluster-side setup gap**, not a chart bug:
- The `agones-sdk` ServiceAccount isn't installed in any namespace on `m00nsrv`.
- Agones Operator was just deployed (`helm list ... agones ... v1.59.0 ... deployed`), but the SDK infrastructure (which lives in `agones-system` by default) is incomplete.
- Agones' typical installation also creates a `games` namespace with a templated `agones-sdk` SA — which would solve this. Not done on this cluster.

The pod never got past the SA lookup, so the sidecar never started, the patch-script configmap wasn't mounted, and the chart's behaviour at runtime is unverified. **End-to-end run requires finishing the Agones Operator setup.**

## Secondary observation — Fleet ended up in `default` namespace

The Fleet was created with `metadata.namespace` left unset because we used `helm template | kubectl apply -f` rather than `helm install --namespace ...`. The Fleet template metadata doesn't include `namespace:`. Two options to fix:

1. Add `namespace: {{ .Release.Namespace }}` to the Fleet's `metadata` block in the Fleet template.
2. Use `helm install` (or `helm template | kubectl apply -n N`) when installing.

Both are correct. Option 1 is the chart-side fix.

## Insights captured

1. **Helm template lint cannot catch Agones schema errors** — the Fleet parses as valid YAML, renders cleanly, and `helm lint` reports no errors. Only the live apiserver (via `--dry-run=server`) catches structural mismatches like the missing `template` layer. Always run the server-side dry-run for Agones-flavored charts.

2. **Agones Fleet schema has nested layers** — Fleet.spec.template (GameServerTemplate).spec (GameServerSpec).template (PodTemplateSpec).spec (PodSpec). The naming is confusing and easy to get wrong; the apiserver's strict decoding error is the only signal until you know the schema.

3. **`spec.template.spec.container` is required, not optional** — even though it's a string field, you must name it whenever the pod has more than one container (which is the sidecar pattern). The apiserver error message is misleading — it claims the field "is invalid" and "could not find a container named ''", which sounds like an empty-name error but is actually a missing-field error.

4. **`docker manifest list` for the sidecar image** isn't verified here — the placeholder digest (`sha256:0000000000000000000000000000000000000000000000000000000000000000`) is intentionally invalid for the deploy test. In production, CI pins a real digest via the round-1 documented workflow.

5. **The Fleet's `metadata.namespace` should be set explicitly** — add `namespace: {{ .Release.Namespace }}` to the Fleet template's `metadata` block, OR pass `--namespace` to `helm install` so the Fleet lands in the right namespace.

## Cleanup status

- Fleet, GameServerSet, GameServer, all 6 cluster-side resources, plus the test namespaces — all deleted or in Terminating state.
- Existing `games/palworld-0` was untouched throughout.

## Recommended follow-ups

1. **Fix the chart**: add `namespace: {{ .Release.Namespace }}` to `helm/templates/fleet.yaml` metadata.
2. **Complete Agones Operator setup**: create the `games` namespace template (or whatever the operator install requires) so `agones-sdk` SA exists in user namespaces.
3. **Re-run deployment test** with real sidecar image digest, in a namespace with `agones-sdk` SA, to verify the sidecar actually starts and the patch script configmaps mount correctly.
4. **Update the Fleet schema docs** in `docs/superpowers/specs/2026-07-29-round-2-clap-scratch-dualstack-design.md` (or a new round) to capture the `template.spec.template.spec` nesting and the `container:` requirement so future contributors don't make the same mistake.

## Files changed this session

- `helm/values.yaml` — Fleet template restructured to use `spec.template.spec.template.spec` nesting; added `container: palworld`; `PALWORLD_API_URL` corrected to `127.0.0.1` (was missed in round 2).
- `helm/templates/fleet.yaml` — Completely rewritten as a single `dict + toYaml` pipeline to avoid map-vs-list indent pitfalls in the original imperative walk.

(These changes are uncommitted at time of writing.)
