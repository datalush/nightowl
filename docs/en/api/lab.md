# Lab recreation, reset, and version record

The lab is a k3d cluster plus external shared pieces. Nothing here provisions credentials: S3 secrets are applied as an imperative ephemeral Secret at lab time, never committed (see the secrets-at-rest discussion).

## Recreate from scratch

```bash
# 1. Cluster: 1 server + 3 agents (placement across nodes is observable).
k3d cluster create lab --servers 1 --agents 3
kubectl create namespace fluss
kubectl create namespace operator-dev

# 2. ZooKeeper (external to the operator; the operator never provisions it).
helm repo add bitnami https://charts.bitnami.com/bitnami
helm install zk bitnami/zookeeper \
  --namespace fluss --version 0.15.0

# 3. Reference Fluss (Helm chart path; the operator never touches this namespace).
helm repo add fluss https://downloads.apache.org/fluss/helm-chart
helm install fluss fluss/fluss \
  --namespace fluss --version 1.0.0

# 4. Operator under test (from this repo; runs against operator-dev).
kubectl apply -f deploy/crd.yaml
RUST_LOG=info ./target/debug/nightowl --namespace operator-dev
```

Wait for `zk-zookeeper-0`, `coordinator-server-0`, and the three `tablet-server-N` pods in `fluss` before running operator tests.

## Host-side access for tests (temporary, remove afterwards)

Fluss advertises stable pod DNS, which the host cannot resolve, and pod IPs, which the host cannot route. Both need temporary host setup while a test runs:

```bash
# Route to pod CIDR via any k3d node container IP (discover with
# `docker inspect k3d-lab-server-0`).
sudo ip route add 10.42.0.0/16 via <k3d-node-ip>

# One /etc/hosts line per pod under test:
# <pod-ip> <pod>.<headless-svc>.<namespace>.svc.cluster.local
```

Remove both when the test ends (`ip route del`, delete the hosts lines). Test failures that mention name resolution or unreachable pod IPs are host-setup failures, not operator failures. The ephemeral S3 Secret is deleted with the test (`kubectl delete secret … -n operator-dev`).

## Reset procedures

- **Single test**: delete the CR, then all PVCs and the ephemeral Secret in `operator-dev`; stop the operator; remove host route and hosts lines. The namespace must be empty afterwards (`kubectl get all,pdb,pvc,secrets -n operator-dev` shows nothing).
- **Full lab**: `k3d cluster delete lab` and follow “Recreate from scratch”. This wipes the reference cluster and ZooKeeper metadata too — only do it deliberately.

## Version record (verified live 2026-09-27)

| Component | Version |
| --- | --- |
| k3d | v5.9.0 |
| Kubernetes (k3s) | v1.35.5+k3s1 (1 server + 3 agents) |
| ZooKeeper (Bitnami chart / app) | zookeeper-0.15.0 / 3.9.5 |
| Reference Fluss (chart / app) | fluss-1.0.0 / `ghcr.io/midnattsol/fluss:1.0.0-midnattsol.3` (rolled 2026-09-28, was `1.0.0-midnattsol.2`) |
| Operator image under test | `ghcr.io/midnattsol/fluss:1.0.0-midnattsol.3` via CR `spec.version` |
| Remote storage | External RustFS 1.0.0 (out-of-band lab hardware) |

Refresh this table whenever the lab moves. The reference `fluss` namespace is a fixed Helm install for comparison; operator tests run exclusively in `operator-dev`.

## Lab metrics (Prometheus)

A minimal Prometheus (`prom/prometheus:v3.5.0`, Deployment + Service in the `monitoring` namespace, manifests kept out of this repo) scrapes Fluss pods via pod discovery filtered on the `prometheus.io/scrape=true` and `prometheus.io/port=9249` annotations. Enable the reporter per cluster through existing API — no operator change needed:

```yaml
configurationOverrides:
  metrics.reporters: prometheus
coordinator:
  podTemplate:
    annotations:
      prometheus.io/scrape: "true"
      prometheus.io/port: "9249"
tabletServers:
  podTemplate:
    annotations:
      prometheus.io/scrape: "true"
      prometheus.io/port: "9249"
```

The scrape job (`fluss-pods`) keeps only port-9249 targets and rewrites `__address__` to pod-IP:9249. Verified 2026-09-27: 3/3 pods `up`, 1131 `fluss_*` series queryable, coordinator gauges reflecting the live test (`activeTabletServerCount=2`, `tableCount=1`). After editing the scrape ConfigMap, reload with `POST /-/reload` (volume sync can lag ~1 min). Note: the Bitnami `prometheus` chart was unusable (its pinned image tag does not resolve); the hand-written manifests above are the lab fixture.

## Fluss Gateway (ship-checked, not deployed by default)

The upstream Gateway does **not** ship inside `apache/fluss:1.0.0` — it is a separate distribution: container `apache/fluss-gateway:1.0.0`, configured purely by environment (`FLUSS_GATEWAY__CLUSTER__DEFAULT__BOOTSTRAP__SERVERS` pointing at an operator-managed coordinator). Ship-checked 2026-09-27 against an operator cluster: Deployment (stock image, one env var) plus ClusterIP Service; `/health` and `/ready` OK; log-table create plus 3/3 appends, PK-table create plus 2/2 upserts, and table describe all over plain HTTP. Verdict: direct use, no fork and no source build needed. Known preview limits (1.0): trust mode only (no auth/TLS), no record reads (lookups/scans need a native client), at-least-once writes. Sources: [Gateway](https://fluss.apache.org/docs/next/gateway), [Deploying](https://fluss.apache.org/docs/install-deploy/deploying-gateway/).

## Bounded workload shape (Gateway writes plus native verify)

The reproducible workload used for evidence: one ephemeral Gateway Deployment beside the test cluster (stock image, bootstrap env, ClusterIP Service, port-forward for host access), DB/table creation plus row batches over plain REST (check `success_count`/`error_count` and per-entry `successes`/`failures`; an HTTP 200 can carry partial failures), then read-back and integrity verification with a native client. Verification must be idempotent by key set, never by physical count: Gateway delivery is at-least-once and retries can duplicate log appends. Verified 2026-09-27: 200 log appends plus 100 PK upserts written (300/300 successes), 300/300 verified native (PK by lookup content, log by dense per-bucket offsets plus content edges). Lab gotcha: arrow `Debug` rendering truncates long columns (`...80 elements...`), so never parse record Debug output for completeness — use offsets and key sets.
