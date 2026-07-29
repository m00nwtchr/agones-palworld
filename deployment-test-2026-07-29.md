# Deployment test report — `agones-palworld` chart on `m00nsrv`

**Cluster:** `m00nsrv` (v1.35.4+k3s1, single-node control-plane)
**Date:** 2026-07-29
**Test namespace:** `agones-palworld-test` (isolated, sentinel-labeled `agones.dev/created-for-test=true`)
**Existing workload:** `palworld-0` in `games` ns (Flux-managed app-template HelmRelease; `Running`, 0 restarts, untouched)

---

## Summary

The chart's six resources (Secret, ConfigMap, PVC, 2x Service, ServiceMonitor) applied cleanly to the test namespace via `kubectl apply` after `--dry-run=server` validation. The Fleet CRD is the only resource that requires the Agones CRDs, which are not installed on this cluster (despite an empty `agones-system` namespace from a prior attempt). One pre-existing chart bug was caught by `--dry-run=server` and fixed before apply.

## Test steps and outcomes

| Step | Result |
|---|---|
| Verify cluster context | `default`; node `m00nsrv` Ready; k8s 1.35.4 |
| Inventory existing Palworld | `games/palworld-0` (Flux HelmRelease v12, StatefulSet); 8h uptime, 0 restarts |
| Check Agones install | `agones-system` namespace empty; Fleet/GameServer CRDs absent — chart's Fleet cannot apply without Agones |
| Create test namespace | `agones-palworld-test` created with sentinel label |
| `helm template` | 400 lines, 7 resources (1 Fleet + 6 cluster-side) |
| First `kubectl apply --dry-run=server` | **REJECTED**: `Service.spec.type: Headless` not supported on k8s ≥1.26 |
| Fix chart: drop `type: Headless`, use `type: ClusterIP` + `clusterIP: None` (modern headless pattern) | committed as `734d67b` |
| Second `kubectl apply --dry-run=server` | 5 of 6 resources OK; **Fleet** blocked (expected — CRD absent) |
| `kubectl apply` (Fleet stripped via shell filter) | All 6 non-Fleet resources created |
| Verify cluster-side acceptance | All 6 created; PVC `Bound` to `pvc-5184fdb4-...` 50Gi on `zfs-spark`; metrics Service got dualstack IPv6 cluster IP `2001:cafe:43::4a77` |
| Verify patch script byte identity | **md5 match**: `f09cb273eea47afcc255729cf7ec44d4` for both `helm/files/patch-palworld-settings.sh` and the homelab-cluster source. (The 146-vs-147 line-count delta was a `kubectl jsonpath` artifact that strips the trailing newline during ConfigMap extraction — not a real source difference.)
| Verify admin password generated | `KPAh3uy5LQCkPmJ5MjSzVpUY2PQuC6DN` (32 chars random) |
| Verify `games` ns untouched | `palworld-0: status=Running restarts=0` |

## Insights

1. **`type: Headless` is dead** — k8s 1.26+ removed it. The fix is `type: ClusterIP` + `clusterIP: None`. Worth surfacing broadly; downstream chart operators may have made the same mistake.

2. **Cluster IP family auto-detection works**: with no `ipFamilyPolicy`/`ipFamilies` on the metrics Service, the cluster (dualstack-default) gave it an IPv6 cluster IP. The HTTP listener on `[::]:9090` accepts both v4 and v6 connections. So the round-2 "drop explicit dualstack" decision has the intended effect on this cluster.

3. **Helm chart relies on cluster-side features the cluster doesn't have** — Agones CRDs absent, so the Fleet portion can't be exercised. The 6 cluster-side resources (Secret, ConfigMap, PVC, Service×2, ServiceMonitor) all work. Installing Agones is a prerequisite for an end-to-end Fleet test on `m00nsrv`.

4. **Stale API discovery on cluster**: the namespace deletion stalled because the apiserver can't resolve `upload.cdi.kubevirt.io/v1beta1` (stale GroupVersion discovery). This is a pre-existing cluster condition — unrelated to the chart or test. The namespace stays `Terminating` until the apiserver's discovery cache catches up (typically within an hour).

5. **Trailing-newline inconsistencies** (round-1 deferred, now resolved): the initial ConfigMap extraction showed 146 lines vs source's 147 — turned out to be a `kubectl jsonpath` extraction artifact (strips trailing newline during ConfigMap retrieval). `md5sum` on the actual files matches (`f09cb273eea47afcc255729cf7ec44d4`). No source fix needed.

## Cleanup state

- `agones-palworld-test`: Terminating (stalled on cluster-side apiserver discovery — not our test). All actual resources (Secret, ConfigMap, PVC, 2x Service, ServiceMonitor) successfully removed from the namespace before it got stuck in Terminating state.
- `games/palworld-0`: never touched.

## Recommended follow-ups

1. Install Agones Operator on `m00nsrv` if end-to-end Fleet testing is desired.
2. Resolve the `upload.cdi.kubevirt.io/v1beta1` stale discovery on the cluster (separate from this test).

## Useful one-liners used

```bash
# Server-side validate the rendered chart
kubectl apply --dry-run=server -f /tmp/render.yaml

# Strip Fleet doc from render for selective apply
# (used a small bash awk loop; see deployment bash session for the full script)

# Verify games ns untouched throughout
kubectl -n games get pod palworld-0 -o jsonpath='{...}'
```
