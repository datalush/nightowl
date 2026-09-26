# Status and conditions

`status` describes observed state. It is **not** supplied by the person creating a `FlussCluster`. The Rust API defines the following shape, but the current watcher does not populate the status subresource.

```yaml
status:
  observedGeneration: 3
  observedConfigHash: "sha256:example"
  observedVersion: "1.0.0"
  clusterHealth:
    status: GREEN
    numReplicas: 120
    inSyncReplicas: 120
    numLeaderReplicas: 40
    activeLeaderReplicas: 40
  coordinatorEndpoints:
    - production-coordinator-0.production-coordinator-hs.data.svc.cluster.local:9124
    - production-coordinator-1.production-coordinator-hs.data.svc.cluster.local:9124
  coordinator:
    desired: 2
    ready: 2
    activePod: production-coordinator-0
  tabletServers:
    desired: 3
    ready: 3
    pods:
      - name: production-tablet-server-0
        ready: true
        assignedTablets: 40
        replicaHealth:
          numReplicas: 40
          inSyncReplicas: 40
          numLeaderReplicas: 13
          activeLeaderReplicas: 13
  conditions:
    - type: FlussReachable
      status: "True"
      reason: CoordinatorResponding
      message: A CoordinatorServer responded to a Fluss admin request.
      evidence:
        - "2 CoordinatorServers configured"
      lastTransitionTime: "2026-09-25T12:00:00Z"
```

This is a **shape example**, not output produced by the current Operator. Endpoint names and health counts are illustrative; the example shows only one of three pod entries for brevity. In particular, `observedVersion` must come from evidence of the running cluster, not just a copy of `spec.version`.

| Field | Type | Meaning |
| --- | --- | --- |
| `observedGeneration` | integer, optional | The last CR generation whose desired state has actually been handled. |
| `observedConfigHash` | string, optional | Hash of the rendered configuration actually observed. |
| `observedVersion` | string, optional | Version confirmed by observation, not simply requested. |
| `clusterHealth` | optional object | `GREEN`, `YELLOW`, `RED`, or `UNKNOWN`, plus global replica/ISR/leader counts. |
| `coordinatorEndpoints` | list of strings | Endpoints clients can use to discover the Coordinator. |
| `coordinator` | optional object | Desired/ready replicas and optional `activePod`. |
| `tabletServers` | optional object | Desired/ready replicas and optional per-pod `assignedTablets`/`replicaHealth`. |
| `conditions` | list | Independent operational statements with evidence and transition time. |

Each condition has `type`, `status`, `reason`, `message`, `evidence`, and `lastTransitionTime`. The schema restricts `status` to `"True"`, `"False"`, or `"Unknown"`; `type` is also an enum: `Ready`, `Progressing`, `Upgrading`, `Stalled`, `Degraded`, `Adoptable`, `KubernetesResourcesReady`, `ZooKeeperReachable`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy`, or `OperationBlocked`. These checks are **proposed behavior**, not a guarantee that they run today. `lastTransitionTime` is currently a string; formatting and transition semantics need implementation.

Fluss 1.0's `getClusterHealth()` supports global counters. The per-pod `assignedTablets` and `replicaHealth` fields require the proposed per-server Admin read API; they must remain absent while unavailable, not be invented from Pod readiness.

Kubernetes Pod readiness alone does not prove Fluss health. A future `RemoteStorageReady=True` needs evidence of actual remote operations, and any decision to restart or scale in needs stronger Fluss-specific evidence than a TCP probe.
