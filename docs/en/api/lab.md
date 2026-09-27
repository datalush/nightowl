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
| Reference Fluss (chart / app) | fluss-1.0.0 / `apache/fluss:1.0.0` |
| Operator image under test | `apache/fluss:1.0.0` via CR `spec.version` |
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
