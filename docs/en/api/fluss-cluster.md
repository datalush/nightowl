# FlussCluster

`FlussCluster` is a namespaced Kubernetes custom resource with `apiVersion: fluss.datalush.com/v1alpha1`. The source of truth for its shape is `operator/src/api.rs`. Its CRD is cluster-wide, while every instance has its own namespace. A future controller will create that instance's resources in the same namespace and derive their names from `metadata.name`.

> **Contract vs. controller:** the Rust types and schema exist. The current controller does not create workloads or enforce the operational rules described below. See [What works today](../current-state.md).

## Top-level spec

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `version` | string | Yes | Desired Fluss version; combined with `image.repository` to select an image tag. |
| `image` | object | Yes | Container image source and pull settings. |
| `zookeeper` | object | Yes | Addresses of an externally managed ZooKeeper ensemble. |
| `coordinator` | object | Yes | CoordinatorServer replica count and pod resources. |
| `tabletServers` | object | Yes | TabletServer replica count, pod resources, and local PVCs. |
| `remoteStorage` | object | Yes | Shared remote storage; currently the API offers an S3-shaped backend. |
| `listeners` | object | No | Internal and client listener names/ports, with in-cluster client access. |
| `podDisruptionBudget` | object | No | Voluntary disruption policies for the two components. |
| `rollingUpgrade` | object | No | Time budgets for a future ordered, Fluss-aware upgrade. |
| `scaleIn` | object | No | Safety policy for removing a TabletServer. |
| `defaults` | object | No | New-table defaults and the cluster's minimum in-sync replica setting. |
| `observability` | object | No | Prometheus reporter intent. |
| `configurationOverrides` | map of strings | No | Additional Fluss `server.yaml` properties. |

Omitting an optional field does **not** imply that the running Operator has chosen a production default. Kubernetes schema validation currently covers field types and some numerical bounds; it does not replace deployment validation.

## Image and ZooKeeper

| Field | Type | Required | Notes |
| --- | --- | --- | --- |
| `image.repository` | string | Yes | For example `apache/fluss`. The desired tag comes from `spec.version`. |
| `image.pullPolicy` | `Always`, `IfNotPresent`, `Never` | No | May be left to the Kubernetes workload default. |
| `image.pullSecrets` | list of names | No | References Secrets in the **same namespace** as the FlussCluster. |
| `zookeeper.addresses` | nonempty list of strings | Yes | Fluss expects a comma-separated `zookeeper.address`; each element should be a reachable `host:port`. |
| `zookeeper.pathRoot` | string | No | Proposed stable default: `/fluss/<namespace>/<name>`. The derivation is not yet implemented. |

Multiple Coordinators using the same ZooKeeper path participate in Fluss 1.0's leader election. A single ZooKeeper pod is still a single point of failure. Both the ZooKeeper path and remote storage location identify existing data; they should remain stable across restarts and eventual upgrades.

## Coordinator and TabletServers

| Field | Type | Required | Notes |
| --- | --- | --- | --- |
| `coordinator.replicas` | integer ≥ 1 | Yes | Fluss 1.0 supports multiple CoordinatorServers with one leader and standby instances. |
| `coordinator.resources` | object | Yes | CPU and memory requests; limits optional. |
| `coordinator.image`, `tabletServers.image` | string | No | Full per-component image override; must agree with the supported version before an upgrade. |
| `coordinator.jvm`, `tabletServers.jvm` | object | No | `heap` (required if present) and optional `extraArgs` strings. |
| `coordinator.storage` | object | No | Optional local PVC settings. ZooKeeper remains the metadata store. |
| `coordinator.scheduling` | object | No | Placement intent; see below. |
| `coordinator.podTemplate` | object | No | Pod labels, annotations and Kubernetes pod security context. |
| `coordinator.configurationOverrides` | map of strings | No | Properties applying only to Coordinators. |
| `tabletServers.replicas` | integer ≥ 1 | Yes | Desired TabletServer processes, not a per-table replication factor. |
| `tabletServers.resources` | object | Yes | CPU and memory requests; limits optional. |
| `tabletServers.storage` | object | Yes | Local, per-TabletServer PVC. Remote storage is configured separately. |
| `tabletServers.scheduling` | object | No | Placement intent; see below. |
| `tabletServers.podTemplate` | object | No | Pod labels, annotations and Kubernetes pod security context. |
| `tabletServers.configurationOverrides` | map of strings | No | Properties applying only to TabletServers. |

`resources.requests.cpu`, `resources.requests.memory`, `resources.limits.cpu`, and `resources.limits.memory` are strings using Kubernetes quantity notation, such as `500m` and `2Gi`. If `limits` is present, **both** CPU and memory are required by the current Rust type. `jvm.heap` is a string such as `1Gi`; the future controller must check that it leaves room under the container memory limit for non-heap memory. `storage.size` is a string such as `20Gi`; `storage.storageClassName` and `storage.dataDir` are optional. `tabletServers.storage.dataDir` identifies the mount path inside a TabletServer pod.

Scheduling supports `spreadAcrossNodes` (boolean), `nodeSelector` (string map), and native Kubernetes `affinity`, `tolerations`, and `topologySpreadConstraints`. All are optional. `podTemplate` holds **only** metadata and `securityContext`, so there is no second node selector or affinity field that can contradict `scheduling`. User pod labels must not override the Operator's ownership or Service-selection labels. The future controller must turn `spreadAcrossNodes` into placement rules; setting it in a CR today does not spread pods. StorageClass expansion, immutability, PVC retention, and scale-in need explicit lifecycle behavior before an Operator can act on changes safely.

## Listeners and disruption policy

`listeners.internal` requires `name` and `port`. `listeners.client` requires `name`, `port`, and `serviceType`; only `ClusterIP` is accepted by this API version. Both listener objects are required if `listeners` is supplied. Ports must be in `1..=65535`, and their names should differ. The future controller must render Services and server configuration from the **same** values and derive `advertised.listeners` from per-pod DNS names; external clients are not modeled yet.

`podDisruptionBudget.tabletServers` requires `enabled` and `maxUnavailable` (integer ≥ 0). Its optional `coordinator` requires `enabled` and `minAvailable` (integer ≥ 1). A tablet budget of `maxUnavailable: 0` blocks eviction-based node drains, but **does not** block direct pod deletion. If `podDisruptionBudget` is omitted, the schema does not currently set a PDB default. The intended safe default in the future controller is `maxUnavailable: 0` for TabletServers.

```yaml
listeners:
  internal: { name: INTERNAL, port: 9123 }
  client: { name: CLIENT, port: 9124, serviceType: ClusterIP }
podDisruptionBudget:
  tabletServers: { enabled: true, maxUnavailable: 0 }
  coordinator: { enabled: true, minAvailable: 1 }
```

## Lifecycle intent

`rollingUpgrade` accepts three required duration strings: `controlledShutdownTimeout` for graceful exit, `recoveryTimeout` for the replacement pod, and `stabilizationWindow` before advancing. The Rust schema does not yet validate their syntax or orchestrate an upgrade. `scaleIn.onNonEmptyTabletServer` currently accepts **only `Block`**: never decrement a StatefulSet if the outgoing TabletServer still hosts replicas. There is no `Force` variant or automatic rebalance in this API.

```yaml
rollingUpgrade:
  controlledShutdownTimeout: 5m
  recoveryTimeout: 30m
  stabilizationWindow: 30s
scaleIn:
  onNonEmptyTabletServer: Block
```

`Block` is a declared policy, **not a working safety gate yet**. Fluss's proposed per-server replica-count API is needed to enforce it. Until the implementation can establish that a scale-in or upgrade is safe, it must leave running resources unchanged and report the blocker in `status`.

## Defaults, observability, and overrides

If `defaults` is present, `buckets` and `logReplicationFactor` are required and must be positive integers; `minInSyncReplicas` is optional:

| Field | Fluss configuration | Scope |
| --- | --- | --- |
| `defaults.buckets` | `default.bucket.number` | Default for new tables. |
| `defaults.logReplicationFactor` | `default.replication.factor` | Default **log** replication for new tables; not the number of TabletServer pods. Must not exceed `tabletServers.replicas` (schema-enforced; the runtime backstop covers CRDs installed before the rule). |
| `defaults.minInSyncReplicas` | `log.replica.min-in-sync-replicas-number` | Server-level log write durability with `acks=all` (not a per-table default). When omitted, the Operator renders the quorum default `floor(RF / 2) + 1` from the **effective** replication factor after overrides are merged (1 when RF is 1); an explicit value must not exceed it. Even replication factors trade availability for durability under this default — documented, not forbidden. Applies to writes that require all acknowledgments; coordinate it with client acknowledgement settings. |

`observability.prometheus` is a boolean, defaulting to `false` in the serialized Rust type. The Prometheus endpoint and Service have not yet been reconciled. `configurationOverrides` at the cluster, Coordinator, and TabletServer levels maps Fluss property names to **string** values, for example `kv.snapshot.interval: "10min"`. Component-level values take precedence over cluster-level values. Avoid putting credentials in any of them. The reconciler **enforces** key ownership: identity, topology, credential, storage-wiring and rendered-default keys (listeners, TabletServer identity, ZooKeeper location, S3 properties, `data.dir`, table defaults) are rejected with a `ConfigBlocked` status naming the key; tuning keys (`kv.*`, `netty.*`, …) and unknown future keys pass through. Dynamic changes and restart-required changes need different workflows.

## Changes to a running cluster

The schema currently accepts updates to these fields. That does **not** make them safe operations. Changing the version, reducing TabletServer replicas, shrinking a PVC, or relocating ZooKeeper/S3 data must not become a blind StatefulSet patch. Until their lifecycle logic exists, those requests must be refused or blocked with a clear condition. Neither existing volumes nor remote objects should be deleted automatically.

See [remote storage and credentials](remote-storage.md) for the S3 fields and [status and conditions](status.md) for observed state.
