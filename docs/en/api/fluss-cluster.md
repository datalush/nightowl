# FlussCluster

`FlussCluster` is a namespaced Kubernetes custom resource with `apiVersion: fluss.datalush.com/v1alpha1`. Its definition lives in `src/api.rs`. The CRD is cluster-wide, but the controller creates each cluster's resources in that cluster's namespace.

> Install the CRD shipped with the operator version you run. See the [local lab](lab.md) to try an example.

## Top-level spec

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `version` | string | Yes | Desired Fluss version; combined with `image.repository` to select an image tag. |
| `image` | object | Yes | Container image source and pull settings. |
| `zookeeper` | object | Yes | Addresses of an externally managed ZooKeeper ensemble. |
| `coordinator` | object | Yes | CoordinatorServer replica count and pod resources. |
| `tabletServers` | object | Yes | TabletServer replica count, pod resources, and local PVCs. |
| `remoteStorage` | object | Yes | Shared remote storage; currently the API offers an S3-shaped backend. |
| `listeners` | object | No | INTERNAL/CLIENT defaults and optional public TLS/SNI endpoints. Existing CRs without public settings remain private. |
| `security.saslPlain` | object | No | Referenced Secret containing native users and an ACL superuser; required when public access is configured. |
| `podDisruptionBudget` | object | No | Voluntary disruption policies for the two components. |
| `rollingUpgrade` | object | No | Time budgets for ordered, Fluss-aware restarts (tablets tail-first, then coordinator, GREEN-gated with stabilization). |
| `scaleIn` | object | No | Safety policy for removing a TabletServer. |
| `defaults` | object | No | New-table defaults and the cluster's minimum in-sync replica setting. |
| `observability` | object | No | Prometheus reporter settings. |
| `configurationOverrides` | map of strings | No | Additional Fluss `server.yaml` properties. |

Omitting an optional field does **not** imply that the running Operator has chosen a production default. Kubernetes schema validation currently covers field types and some numerical bounds; it does not replace deployment validation.

## Image and ZooKeeper

| Field | Type | Required | Notes |
| --- | --- | --- | --- |
| `image.repository` | string | Yes | For example `apache/fluss`. The desired tag comes from `spec.version`. |
| `image.pullPolicy` | `Always`, `IfNotPresent`, `Never` | No | May be left to the Kubernetes workload default. |
| `image.pullSecrets` | list of names | No | References Secrets in the **same namespace** as the FlussCluster. |
| `zookeeper.addresses` | nonempty list of strings | Yes | Fluss expects a comma-separated `zookeeper.address`; each element should be a reachable `host:port`. |
| `zookeeper.pathRoot` | string | No | Defaults to `/fluss/<namespace>/<name>` when omitted. |

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

CPU and memory requests and limits use Kubernetes quantities such as `500m` and `2Gi`. If `limits` is present, both CPU and memory are required. Leave room for off-heap use: the operator blocks a `jvm.heap` larger than the memory request (or limit, if set). `storage.size` also uses a Kubernetes quantity such as `20Gi`. `storage.storageClassName` and `storage.dataDir` are optional; the latter sets the mount path in TabletServer pods.

Scheduling supports `spreadAcrossNodes`, `nodeSelector`, `affinity`, `tolerations` and `topologySpreadConstraints`. `spreadAcrossNodes: true` adds a **soft** hostname spread rule (`ScheduleAnyway`), not a guarantee that replicas occupy distinct nodes. `podTemplate` holds metadata and `securityContext`; operator-managed labels take precedence over user labels.

The operator grows existing PVCs when the StorageClass permits expansion, blocks shrinking or changing the StorageClass, and retains PVCs on scale-in. It checks outgoing TabletServers before reducing replicas (see below).

## Listeners and disruption policy

`listeners.internal` and `listeners.client` default to INTERNAL:9123 and CLIENT:9124. To enable public TLS/SNI access, provide `listeners.external.domain`, `gateway.className`, `tls.secretName` and `security.saslPlain`. The operator waits for the TLS and SASL Secrets before starting public workloads.

See [native external access](native-external-access.md) for DNS and routing. `status.externalEndpoints` reports converged Services and `NativeRoutesProgrammed` reports Gateway API conditions; neither proves external reachability.

`podDisruptionBudget.tabletServers` requires `enabled` and `maxUnavailable` (integer ≥ 0). Its optional `coordinator` requires `enabled` and `minAvailable` (integer ≥ 1). A tablet budget of `maxUnavailable: 0` blocks eviction-based node drains, but **does not** block direct pod deletion. If the section is omitted, the operator creates a TabletServer PDB with `maxUnavailable: 0`; the Coordinator PDB is opt-in. Setting `tabletServers.enabled: false` removes the owned tablet budget.

```yaml
listeners:
  internal: { name: INTERNAL, port: 9123 }
  client: { name: CLIENT, port: 9124, serviceType: ClusterIP }
podDisruptionBudget:
  tabletServers: { enabled: true, maxUnavailable: 0 }
  coordinator: { enabled: true, minAvailable: 1 }
```

## Restarts and scale-in

`rollingUpgrade` sets three durations: `controlledShutdownTimeout`, `recoveryTimeout` and `stabilizationWindow`. The first sets the pod's shutdown grace period (30s when omitted); Fluss handles SIGTERM without a preStop hook. Invalid durations block the operation rather than being guessed.

The restart sequencer deletes one pod at a time. It needs fresh GREEN health before each deletion and waits for recovery and stabilization before proceeding. `scaleIn.onNonEmptyTabletServer` only accepts `Block`; there is no `Force` option or automatic rebalance.

```yaml
rollingUpgrade:
  controlledShutdownTimeout: 5m
  recoveryTimeout: 30m
  stabilizationWindow: 30s
scaleIn:
  onNonEmptyTabletServer: Block
```

The scale-in gate reads current Fluss membership and per-server replica counts before reducing the StatefulSet. Every outgoing TabletServer must be registered and host **zero** replicas. If a server is non-empty or cannot be checked, the operator keeps the existing replica count and reports why. The per-server read requires a compatible Fluss server; stock Fluss 1.0 does not provide it. The operator does not rebalance tablets automatically.

## Defaults, observability, and overrides

If `defaults` is present, `tableBuckets` and `logReplicationFactor` are required and must be positive integers; `minInSyncReplicas` is optional:

| Field | Fluss configuration | Scope |
| --- | --- | --- |
| `defaults.tableBuckets` | `default.bucket.number` | Default **table** buckets (sharding) for new tables — unrelated to any S3 bucket. |
| `defaults.logReplicationFactor` | `default.replication.factor` | Default **log** replication for new tables; not the number of TabletServer pods. Must not exceed `tabletServers.replicas` (schema-enforced; the runtime backstop covers CRDs installed before the rule). |
| `defaults.minInSyncReplicas` | `log.replica.min-in-sync-replicas-number` | Server-level minimum for acknowledged log writes; see below. |

If `minInSyncReplicas` is omitted, the operator renders `floor(RF / 2) + 1`
from the effective replication factor (1 for RF=1). An explicit value must
not exceed RF. With even RF, this default favors durability over write
availability; align it with client acknowledgement settings.

### Metrics

Prometheus reporting is on by default. The operator renders `metrics.reporters: prometheus` and adds scrape annotations to Coordinator and TabletServer pods. `observability.prometheus: false` opts out. It does not create a metrics Service or ServiceMonitor.

### Configuration overrides

Set Fluss properties as **strings** in `configurationOverrides` at cluster or component level, for example `kv.snapshot.interval: "10min"`. Component values take precedence. Do not put credentials here: use Secret references instead.

The operator rejects overrides of keys it owns, including listeners, TabletServer identity, ZooKeeper path, S3 settings, `data.dir` and table defaults. A block names the key in `status`. Other keys pass through to Fluss.

### Applying changes

Dynamic keys supported by Fluss 1.0, such as `kv.snapshot.interval`, can be applied through Admin without a restart when Coordinator and TabletServers request the same value. Applied values appear as **hashes**, never plaintext, in `status.appliedDynamicConfig`.

Other changes use sequenced restarts: the StatefulSets run `OnDelete` and the operator deletes pods one at a time after health checks. A dynamic-key rejection appears as `OperationBlocked` and falls back to a restart attempt; persistent rejections remain visible rather than triggering a restart loop.

### Optional HTTP Gateway

`gateway.enabled: true` creates a Deployment and ClusterIP Service. The image
defaults to `apache/fluss-gateway:<spec.version>` and can be overridden.
`status.gateway` shows ready replicas and the internal URL while enabled.

`gateway.ingress` also creates an Ingress for the requested host and class.
Its TLS Secret must already exist; otherwise `GatewayBlocked` explains the
refusal. The platform provisions DNS, TLS and authentication. Disabling the
Gateway removes only its owned resources.

The HTTP Gateway is incompatible with `security.saslPlain`: it does not forward
the caller's native principal, so the CRD rejects enabling them together. The
SNI Gateway API listener used for native clients is a separate resource.

Wildcard Ingress hosts such as `*.example.com` are supported when the Ingress controller accepts them. The operator only references the TLS Secret; your platform creates it.

## Changes to a running cluster

The CRD accepting an update does not mean every change is safe. Image changes use sequenced restarts, and scale-in requires the fresh per-server check above. PVC shrinking and StorageClass changes are blocked; PVC expansion depends on the StorageClass. Do not change ZooKeeper paths, S3 buckets or prefixes without migrating the data. The operator does not delete PVCs or remote objects for you.

See [remote storage and credentials](remote-storage.md) for the S3 fields and [status and conditions](status.md) for observed state.
