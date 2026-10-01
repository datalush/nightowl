# Status and conditions

`status` describes observed state. It is **not** supplied by the person creating a `FlussCluster`. Fields stay absent until the controller can actually observe them; an absent field is honest, an invented one is not.

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

This is a **shape example**; endpoint names and health counts are illustrative. `observedVersion` must come from evidence of the running cluster, not just a copy of `spec.version`.

## Fluss-side health

When the coordinator answers over the internal listener, the controller also fills `clusterHealth` (global replica/ISR/leader counts from `getClusterHealth`), `coordinatorEndpoints` (coordinator servers seen in membership), `coordinator` (desired vs registered) and `tabletServers` (desired vs registered members, one entry per server), plus the `FlussReachable` and `ClusterHealthy` conditions. Membership is Fluss-observed — strictly more honest than pod Ready for "how many servers serve".

Probes run at most every 60 seconds per cluster (in-memory rate limit, no status churn); between probes the last observed values stand. An unreachable cluster reports `FlussReachable=False` with the cause and leaves `ClusterHealthy` at its previous value — or absent when never observed. Health observation never blocks convergence and never retries hot.

Per-pod `assignedTablets` and `replicaHealth` populate when the server answers `DescribeTabletServers` (fork image `1.0.0-midnattsol.1` and later, verified live 2026-09-27); `coordinator.activePod` stays absent. Against stock Fluss 1.0 they remain absent while unavailable, never invented from Pod readiness.

| Field | Type | Meaning |
| --- | --- | --- |
| `observedGeneration` | integer, optional | The last CR generation whose desired state has actually been handled. |
| `observedConfigHash` | string, optional | Combined hash of the rendered coordinator/tablet settings, including the generated external identity when present. |
| `observedVersion` | string, optional | Version confirmed by observation, not simply requested. |
| `appliedDynamicConfig` | map, optional | Applied dynamic keys to value-hashes (hashes only, never values); records what Admin already holds. |
| `clusterHealth` | optional object | `GREEN`, `YELLOW`, `RED`, or `UNKNOWN`, plus global replica/ISR/leader counts. |
| `coordinatorEndpoints` | list of strings | Observed internal coordinator endpoints; external clients use the public bootstrap. |
| `externalEndpoints` | list of objects | Public addresses with converged, owned Services; not a reachability check. |
| `coordinator` | optional object | Desired/ready replicas and optional `activePod`. |
| `tabletServers` | optional object | Desired/ready replicas and optional per-pod `assignedTablets`/`replicaHealth`. |
| `gateway` | optional object | Desired/ready Gateway replicas plus the in-cluster URL; absent unless requested. |
| `conditions` | list | Independent operational statements with evidence and transition time. |

Each condition has `type`, `status`, `reason`, `message`, `evidence`, and `lastTransitionTime`. The schema restricts `status` to `"True"`, `"False"`, or `"Unknown"`. `NativeRoutesProgrammed=True` means the Gateway reports Programmed and every TLSRoute has fresh Accepted/ResolvedRefs conditions, **not** that DNS, TLS or a remote client has succeeded. `KubernetesResourcesReady`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy`, `S3CredentialsStale`, `Stalled` (sequenced-restart stalls), `DataAtRisk` (RED health with hosted replicas but zero active leaders: observe-and-report only) and `OperationBlocked` (dynamic-config rejections) also run today. Other enum values remain reserved for later work.

Fluss 1.0's `getClusterHealth()` supports global counters. The per-pod `assignedTablets` and `replicaHealth` fields require the per-server Admin read API: absent upstream in 1.0, provided by the fork image and observed into status when the server answers; they must remain absent while unavailable, not be invented from Pod readiness.

Kubernetes Pod readiness alone does not prove Fluss health. `RemoteStorageReady=True` carries reference-resolution evidence (referenced Secret with its keys, or ServiceAccount, present) — it does not assert remote operations, and any decision to restart or scale in needs stronger Fluss-specific evidence than a TCP probe.
