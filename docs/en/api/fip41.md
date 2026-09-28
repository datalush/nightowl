# FIP-41 audit and Admin capability matrix

FIP-41 ([wiki](https://cwiki.apache.org/confluence/spaces/FLUSS/pages/421957775/FIP-41+Fluss+Kubernetes+Operator), status **accepted**, umbrella [apache/fluss#3787](https://github.com/apache/fluss/issues/3787)) proposes a Java (JOSDK) operator from a dedicated repository. This operator is intentionally different: **Rust** (`kube-rs` + `fluss-rs`), developed in this repository. The Rust viability decision is affirmative — the full reconcile loop (bootstrap, config, storage, PDBs, health, conditions) is verified live in k3d across the runs recorded in these docs. Language choice does not remove the one load-bearing server-side gap below.

Conventions: **supported** (implemented and lab-verified), **blocked** (needs upstream work; fail-closed meanwhile), **intentionally different** (deliberate divergence with reason), **deferred** (tracked elsewhere, not started).

## Requirement matrix

| FIP-41 requirement | Verdict | Notes |
| --- | --- | --- |
| Java/JOSDK implementation | Intentionally different | Rust operator; viability proven by verified runs. |
| `fluss.apache.org/v1alpha1` CRD group | Intentionally different | Ours is a distinct group to avoid colliding with any future official CRD. |
| Coordinator replicas = 1 | Supported | Single coordinator enforced. |
| Ordinal-derived `tablet-server.id`, stable id↔PVC↔data binding | Supported | Verified across restarts and rollouts. |
| PVCs via `volumeClaimTemplates`, Retain/Retain, never auto-delete PVCs/S3/ZK | Supported | Verified; shrink and StorageClass change refused. |
| Bootstrap order: config → coordinator → tablets → Ready | Supported | Verified, including restart idempotency. |
| PDB `maxUnavailable: 0` on tablets; operator rolls via direct delete | Supported | Verified; PDB blocks eviction only, direct delete unaffected. |
| Scale-in safety gate (refuse non-empty server) | Supported (partial) | Gate consults a fresh per-server read (fork image): registered-and-empty converges, anything else fails closed with the exact blocker. Live removal workflow pending in `g8qz`. |
| Rolling upgrade: tablets first, per-server `serverGreen` gate, `Stalled` state | Supported (partial) | Sequenced restarts run (tail-first tablets, GREEN-gated, `Stalled` with evidence); version-pair preflight plus upgrade orchestration in `cm74`. |
| Dynamic vs restart-inducing config classification | Supported (partial) | Key ownership enforced, unknown keys fail closed; dynamic apply via Admin is pending. |
| Rendered config content-hash driving restarts | Supported | Config hash in pod template directs the restart sequencer (StatefulSets run OnDelete; no Kubernetes-native rolling). |
| Storage resize (expand in place, orphan-recreate for template) | Supported (partial) | Expansion verified live; shrink refused. Orphan-recreate path accepted but not yet orchestrated. |
| Conditions-only lifecycle (`Ready`, `Progressing`, `Upgrading`, `Stalled`, `Degraded`) | Intentionally different | Area conditions (`KubernetesResourcesReady`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy`, `S3CredentialsStale`) plus `Stalled` for sequenced-restart stalls; the rest belong to the upgrade work. |
| Rendered `server.yaml` in a Secret | Intentionally different | Rendered config is a ConfigMap holding **markers** (`${directory:…}`), never credential values; values stay in the mounted Secret. Strictly stronger than FIP-41's Secret-with-values. |
| Structured listeners + advertised DNS derivation | Supported | Internal/client listeners drive Services and advertised addresses. |
| Operator never triggers `rebalance()` in v1alpha1 | Supported | The operator never calls rebalance; evacuation stays admin-driven. |
| CEL/admission validation instead of optional webhook | Intentionally different | Schema rules in the CRD; no webhook to operate. |
| Cluster-wide watch by default, minimal RBAC | Supported | Verified with an unprivileged ServiceAccount; cross-namespace isolation tested. |
| ZooKeeper external; no ZK management | Supported | ZK address is configuration, never provisioned. |
| Table defaults (buckets, RF, min-ISR) | Intentionally different (addition) | FIP-41 has no defaults section; ours renders `default.bucket.number` and replication guards. |
| Structured S3 remote storage + delegation | Intentionally different (addition) | Beyond FIP-41's plaintext-key limitation; Secret markers plus AssumeRole/GetSessionToken verified. |
| Operator leader election via Lease | Deferred | Single replica today; tracked with packaging. |
| Prometheus metrics endpoint | Supported (partial) | Reporter key plus scrape annotations wired, default-on with opt-out; metrics Service/ServiceMonitor and reconciler-own metrics stay deferred with packaging. |
| In-place adoption (Path A) / drain-mode replacement (Path B) | Deferred | Tracked separately; the operator never adopts foreign resources. |
| Lake tiering job, table management, backup/restore orchestration | Out of scope | Matches FIP-41 non-goals; restore is observe-and-report. |

## Direct answers required by the audit

- **`describeTabletServers`**: does not exist in Fluss 1.0; implemented in our fork image (`1.0.0-midnattsol.1`, ApiKey 1067) and observed live in-cluster 2026-09-27 (3/3 servers with counters, cluster GREEN). Upstream asks stay open ([apache/fluss#3743](https://github.com/apache/fluss/issues/3743), [apache/fluss#3570](https://github.com/apache/fluss/issues/3570)). Scale-in of non-empty servers and per-server upgrade gating still stay refused until wired to it.
- **`listServerTags`**: does not exist (FIP-41 states tags can be added/removed but not listed). No operator flow depends on it yet.
- **Upgrade health signals**: cluster-wide `getClusterHealth()` exists and is used (GREEN/YELLOW/RED/UNKNOWN drives the `ClusterHealthy` condition). Per-server health does not exist — same gap as above. No auto-rollback exists anywhere by design.
- **Drain mode / `decommissionServer` / min-ISR-aware rebalance**: none exist server-side; all deferred with adoption.

## Admin capability matrix (`fluss-rs` 1.0.0 vs Fluss 1.0 server)

| Capability | Server 1.0 | `fluss-rs` 1.0 | Operator use | Verdict |
| --- | --- | --- | --- | --- |
| `getClusterHealth` | Yes | `get_cluster_health` | `ClusterHealthy` condition | Supported |
| `describeTabletServers` (per-server replicas/ISR/leaders) | No (stock) / Yes (fork image) | Yes (fork pin) | Observed into status; scale-in gate, `serverGreen` gating pending | Partial |
| `listServerTags` | No | No | Rebalance bounding (v1beta1) | Blocked |
| `addServerTag` / `removeServerTag` | Yes | Yes | Unused (never drive rebalance) | Available |
| `rebalance` / `listRebalanceProgress` / `cancelRebalance` | Yes | Yes | Deliberately unused | Intentionally different |
| `describeClusterConfigs` / `alterClusterConfigs` | Yes | Yes | Ownership enforced; dynamic apply pending | Partial |
| KV snapshot read + leases (`getLatestKvSnapshots`, metadata, lease acquire/release/drop) | Yes | Yes | Lab verification; operator never moves bytes | Supported |
| `GetFileSystemSecurityToken` + client renewal | Yes | `SecurityTokenManager` | Client-token flow verified | Supported |
| Table/database/partition ACL management | Yes | Yes | Lab tooling only; no table CRD | Intentionally different |
| `getServerNodes` | Yes | Yes | Bootstrap/metadata | Available |
| Offsets, lake snapshots, remote-log manifests, producer offsets | Yes | Yes | Unused | Available |

Sources: [FIP-41 wiki](https://cwiki.apache.org/confluence/spaces/FLUSS/pages/421957775/FIP-41+Fluss+Kubernetes+Operator), [umbrella issue](https://github.com/apache/fluss/issues/3787), [`DescribeTabletServers` upstream](https://github.com/apache/fluss/issues/3743), [`fluss-rs` 1.0.0 `admin.rs`](https://docs.rs/fluss), [Fluss 1.0 S3 configuration](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/).
