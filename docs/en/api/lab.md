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
Re-apply `deploy/crd.yaml` after any API change (new status fields or condition
types): the apiserver rejects status writes with unknown enum values (422) or
silently prunes unknown fields, and the failure looks like the operator being
down rather than a stale CRD.

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

## Version record (environment verified 2026-09-27; restore drill 2026-09-28)

| Component | Version |
| --- | --- |
| k3d | v5.9.0 |
| Kubernetes (k3s) | v1.35.5+k3s1 (1 server + 3 agents) |
| ZooKeeper (Bitnami chart / app) | zookeeper-0.15.0 / 3.9.5 |
| Reference Fluss (chart / app) | fluss-1.0.0 / `ghcr.io/midnattsol/fluss:1.0.0-midnattsol.3` (StatefulSet template; reference rollout remains blocked) |
| Previous operator-owned drill | `rst-drill`, image `ghcr.io/midnattsol/fluss:1.0.0-midnattsol.4`; snapshot download worked but recovery failed after disk loss |
| Verified operator-owned restore drill | `rst-drill5`, image `ghcr.io/midnattsol/fluss:1.0.0-midnattsol.5`, built from fork `develop` `d2a1f4382629f8057b8fb0a2cf6b74c90cfb51f3` |
| RF=2 follower drill | `rst-rf2`, image `.5`, dedicated remote prefix; replacement follower promoted and restored 50/50 KV rows |
| Remote storage | External RustFS 1.0.0 (out-of-band lab hardware) |

Refresh this table whenever the lab moves. The reference `fluss` namespace is a fixed Helm install for comparison; operator tests run exclusively in `operator-dev`.

### RF=1 disk-loss restore, 2026-09-28

In a fresh `rst-drill5` cluster with a dedicated `clusters/operator-dev/rst-drill5` S3 prefix, Gateway wrote 50 KV rows in `rst5.kv`. A native client verified **all 50 keys and their exact string values**. RustFS snapshot 0 then recorded `row_count=50`, `log_offset=50`, `_METADATA` and `_WRITER_STATE`.

The tabletserver PVC (`a09176b1-b65f-4736-a0ea-f2cae95efbf3`, volume with the same suffix) and pod were deleted. Kubernetes created a new PVC (`87bd588b-c734-4196-b6e3-fd95f9a279ec`) and pod; Fluss logged snapshot download and recovery from offset 50 on the replacement disk. The native client again verified **the same 50 keys and exact values**. Ten subsequent writes succeeded; all **60/60** keys and values were read back. The recovered tablet completed a further remote snapshot (ID 1).

Fluss reported `GREEN`, 1/1 leader and replicas, together with persistent `data_at_risk=true`; Night Owl pinned to `d2a1f438` reported `DataAtRisk=True` / `SnapshotRecoveryUnverified` even with a healthy live leader. The marker is intentional: RF=1 cannot prove that no acknowledged writes existed beyond the latest durable snapshot or remote-log offset. This drill does not establish zero-RPO recovery. Snapshots produced by `.4` lack the writer checkpoint required to restore an empty log with `.5`; the test created its snapshot on `.5`.

### RF=2 follower replacement and promotion, 2026-09-28

The separate `rst-rf2` cluster used image `.5`, a dedicated `clusters/operator-dev/rst-rf2` S3 prefix, two tabletservers and a one-bucket RF=2 table `rf2drill.kv`. Tabletserver 0 led and tabletserver 1 followed at ISR 2/2. A native client verified 50/50 exact keys and values; remote snapshot 0 contained 50 rows at offset 50 with `_WRITER_STATE`.

Only the follower's disk and pod were replaced: PVC `95695e67-ee41-4519-abef-15c4dce13d59` became `d9a3ca49-3a44-42c9-878d-7747fbac1c95`, and the follower returned to ISR 2/2. Deleting leader 0's **pod** (not its PVC) then caused tabletserver 1 to become leader at epoch 1. It downloaded snapshot 0 from S3, recovered KV from offset 50, and served all 50 exact keys and values. The Gateway's first ten post-failover requests reported errors; after retrying, ten new keys were written and **60/60** exact values verified. The promoted leader completed snapshot 1 (`row_count=60`, `log_offset=80`, writer checkpoint present); offsets count WAL activity, not distinct KV keys. Final observed health: GREEN, ISR 2/2, `data_at_risk=false`, Night Owl `DataAtRisk=False`. This verifies follower replacement and promotion, not simultaneous loss of both replicas.

## Native external listener via Envoy, 2026-09-29

Verified in a separate k3d cluster `native-access` (one server, two agents), without
changing the reference lab. Envoy Gateway Helm **1.5.0**, Gateway API `Gateway/v1`
and `TCPRoute/v1alpha2`, Fluss image **1.0.0-midnattsol.5**, and the operator's
existing native SDK revision **d2a1f438**. This image predates the abandoned custom
JWT/OAUTHBEARER work; no Fluss core changes were needed for external routing.

Cluster `native-test/externaltest` used two coordinators, initially two tablets,
RF=2 and two table buckets. Separate ZooKeeper 3.9.3 and ephemeral RustFS 1.0.0
provided lab dependencies. Envoy's LoadBalancer was exposed by k3s ServiceLB on
the Docker node address `172.18.0.2`: coordinators 23000/23001 and tablets
24000/24001, later 24002. No pod-CIDR routing or hosts-file changes were added.

The host-side test used in the earlier per-port drill (superseded by the SNI design):

- Discovered the externally advertised coordinator and both tablet addresses.
- Created `operator_external.phase1`, wrote 20 KV rows, and verified all 20 exact
  values through Envoy.
- Replaced tablet-0's pod, retained its PVC, then read all 20 original values
  without rewriting them. Pod UID changed from `0438662e-fd6a-41e4-9be0-58e73a66846f`
  to `4d3dff83-d970-49b1-b03a-3cfc5547e94f`.
- Deleted active coordinator-0. An immediately started fresh native client with
  both bootstrap addresses discovered coordinator-1 at port 23001 and read 20/20
  original values. This proves leader rediscovery, not uninterrupted requests on
  an already-open connection.
- Scaled tablets from two to three and regenerated/applied the GitOps routes.
  Metadata included tablet-2 at 24002; the old endpoints and existing pod identities
  remained stable, and 20/20 original values were still readable.

Final observed health: GREEN, 4/4 replicas in sync, 2/2 leaders active,
`dataAtRisk=false`; status contained five configured external mappings. Internal
Admin health checks remained functional. ACL/SASL and remote-file client reads
were not exercised in this phase.

Additional discovery checks: bootstrapping separately through the active
coordinator and each of the three tablet routes succeeded with 20/20 reads. The
standby coordinator alone returned `NotLeader` (code 65); the full coordinator
bootstrap list succeeded. The external configuration therefore publishes all
coordinators rather than depending on a randomly balanced coordinator Service.

Two setup findings: Envoy reserves 19000/19001 internally, so those ports cannot
be used for these public listeners despite route acceptance. The existing external
lab object store was unreachable from the isolated network; a fresh cluster with
local RustFS was used for the successful data tests. See
[native external access](native-external-access.md) for reproduction.

## Lab metrics (Prometheus)

### Single-IP TLS/SNI native access, 2026-10-01

Isolated k3d `native-sni` (one server, two agents; the reference lab remained
untouched): Envoy Gateway Helm 1.9.1 with `TLSRoute/v1`, Envoy sidecars 1.33.4,
Fluss image `1.0.0-midnattsol.5` and Rust/Java TLS clients from `feat/clients`.
The external DNS `fluss.172.19.0.2.sslip.io` and generated `coordinator-N`/
`tablet-N` subdomains all resolved to the same Docker node IP, port 443.

A CA-signed end-entity certificate covered the base and wildcard domains.
The first self-signed CA mistakenly used as a server certificate was rejected
by the Rust client (`CaUsedAsEndEntity`), confirming certificate validation.
After correcting the chain, both clients established TLS/SNI via an Envoy
passthrough Gateway and per-pod sidecars: Rust wrote and looked up **20/20**
exact values; Java looked up those **same 20/20** values using the bundled
Flink 1.20 client. Deleting the manually applied TLSRoutes/Gateway caused
the operator to recreate them with owner references; both clients still
looked up 20/20 values.

Updating the Secret with a new end-entity keypair signed by the same CA
changed the certificate serial served by a tablet without changing that
tablet pod's UID (SDS file watch). With optional `internalMapping: true`,
the operator produced five CoreDNS rewrite rules. A platform-admin copy
to k3s `coredns-custom` followed by the initial CoreDNS rollout made the
base domain resolve from inside to the bootstrap ClusterIP and
`tablet-0` resolve to its own ClusterIP. That platform copy is not performed
by the operator.

After provisioning `security.saslPlain` from an ephemeral Secret and enabling
native Fluss ACLs, the operator rolled the servers in order. Its tablet health
probe needed to use INTERNAL: an unauthenticated probe against the now-SASL
CLIENT listener could not become Ready. The keys-driven restart sequencer also
needed to record a successful pod deletion, otherwise it deleted the same
tablet again after it became Ready. Both problems were fixed and tested.

The admin used the native Java API to grant Alice READ/DESCRIBE on
`native_sni.phase1`. **Alice read 20/20** through both Java and Rust over
TLS/SNI + SASL. Bob authenticated but was **denied ACL administration with
`AuthorizationException`** and could not read the table. On a fully rolled
cluster the operator's internal Admin still observed GREEN after coordinator
0 answered `NotLeader` and it retried coordinator 1.

Subsequent failure/scale drills used the same authenticated external client:
replacing tablet-0 while preserving its PVC changed its pod UID and Alice
read the original 20/20 values; deleting the active coordinator-1 caused
the next fresh bootstrap to discover coordinator-0 and again read 20/20.
Scaling from two to three tablets created the third owned Service and TLSRoute
automatically; the client discovered `tablet-2` on the same IP:443 and read
20/20. The platform refreshed CoreDNS's optional mapping fragment; after a
CoreDNS rollout, `tablet-2` resolved internally to its own ClusterIP
(`10.43.190.181`), not the external Gateway IP. The platform update and
CoreDNS rollout are **not** performed by the operator. Two Java client Jobs
using the Flink 1.20 bundle also read 20/20 from inside Kubernetes: first
through the public Gateway IP and then through split DNS, from pods on two
different k3d nodes. DNS inside the latter resolved the base to
`native-bootstrap` (`10.43.10.241`) and `tablet-0` to its per-server
ClusterIP (`10.43.6.221`). These are worker-like clients, not Flink jobs.
On 2026-10-01 an actual Flink 1.20 standalone cluster ran with a JobManager
and two TaskManagers on different k3d nodes. A batch SQL job using the Fluss
1.20 connector and the **single public TLS/SNI bootstrap** queried
`SELECT COUNT(*) FROM native_sni.phase1` through SASL and returned **20**;
Flink reported the job `FINISHED` with both tasks finished. This verifies a
real distributed Flink read, but **not** checkpoint recovery, Flink failover,
Spark, external object-store access, or a direct client read of remote snapshot
objects. The lab's Flink deployment and credentials remained outside Git.
See [native external access](native-external-access.md).

One final scale-in from three tablets to two was admitted only after a fresh
Admin check found tablet-2 registered with **zero** hosted replicas. The
StatefulSet removed that pod; then the operator removed only its owned
TLSRoute and Service. Alice still read 20/20 values through the base bootstrap.
The retired tablet PVC was retained.

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
