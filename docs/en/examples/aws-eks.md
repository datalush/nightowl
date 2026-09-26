# AWS EKS

**Workload identity · Amazon S3 · Coordinator HA**

This manifest demonstrates the current `FlussCluster` API for two Coordinators, three TabletServers, and Amazon S3. It is **not deployable by the current watcher**; see [What works today](../current-state.md).

```yaml
{{#include aws-eks.yaml}}
```

## Identity and permissions

The ServiceAccount `fluss-production` must already exist in the `data` namespace. An AWS administrator must configure **IRSA** or an **EKS Pod Identity association** for that ServiceAccount; setting its name in the CR does not provision IAM. The server identity needs permissions for the S3 bucket/prefix and `sts:AssumeRole` on `fluss-clients-read`.

The `roleArn` in `delegation` is the **client-delegation role**, not the ServiceAccount's role. Configure its trust policy to permit the server role to assume it and give it the S3 access required by clients. Scope permissions to the dedicated prefix where possible; if the bucket uses SSE-KMS, account for KMS permissions as well. The bucket, role, ZooKeeper ensemble, and StorageClass must already exist.

Fluss 1.0 uses the AWS default credential chain for IRSA; EKS Pod Identity also exposes credentials through that chain. The exact Pod Identity + Fluss token issuance path still needs an integration test before it can be described as verified. [Fluss S3 reference](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/) · [EKS workload identity](https://docs.aws.amazon.com/eks/latest/userguide/service-accounts.html)

## Operational meaning

`spreadAcrossNodes` expresses intent to avoid putting all replicas on one node; no scheduling policy is rendered by the current program. `jvm.heap` must leave room under the container's memory limit for off-heap use. Listener names/ports would drive both Services and Fluss configuration. The PDB requests `maxUnavailable: 0` for TabletServers; `scaleIn: Block` requires a separate Fluss-aware check, and `rollingUpgrade` timeouts do **not** enable upgrades on their own.

The default log replication factor applies to **new tables**. Running two Coordinators does not make the external ZooKeeper ensemble or S3 bucket highly available by itself. Storage, upgrades, and safe scale-in require the lifecycle work described in [the API reference](../api/fluss-cluster.md#changes-to-a-running-cluster).
