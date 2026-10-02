# Status and conditions

The operator writes `status` from observations; it is not part of the configuration you apply. Fields whose values cannot be observed stay absent.

```yaml
status:
  observedGeneration: 3
  clusterHealth:
    status: GREEN
    numReplicas: 2
    inSyncReplicas: 2
    numLeaderReplicas: 1
    activeLeaderReplicas: 1
  coordinator:
    desired: 1
    ready: 1
  tabletServers:
    desired: 2
    ready: 2
    pods:
      - name: production-tablet-server-0
        ready: true
  conditions:
    - type: FlussReachable
      status: "True"
      reason: FlussReachable
      message: coordinator reachable at production-coordinator-0:9123
      evidence: ["coordinator production-coordinator-0:9123 reachable"]
      lastTransitionTime: "<observed transition time>"
```

This is an excerpt, not a complete status object; the names and counts are illustrative. `observedVersion` is not simply copied from `spec.version`.

## Fluss-side health

When the coordinator answers over the internal listener, the operator records
global replica and leader counts in `clusterHealth`. It also reports observed
coordinator endpoints and registered Coordinator and TabletServer counts.
`FlussReachable` and `ClusterHealthy` reflect the probe. Fluss membership,
not pod readiness alone, determines how many servers are available.

Probes run at most every 60 seconds per cluster (in-memory rate limit, no status churn); between probes the last observed values stand. An unreachable cluster reports `FlussReachable=False` with the cause and leaves `ClusterHealthy` at its previous value — or absent when never observed. Health observation never blocks convergence and never retries hot.

Per-pod `assignedTablets` and `replicaHealth` populate when the server answers `DescribeTabletServers` (available in the compatible fork image, not stock Fluss 1.0). `coordinator.activePod` stays absent; the operator does not infer it from Pod readiness.

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

Each condition has `type`, `status`, `reason`, `message`, `evidence` and `lastTransitionTime`. Status is `"True"`, `"False"` or `"Unknown"`.

`NativeRoutesProgrammed=True` means the Gateway and TLSRoutes report their current programmed/accepted state; it does not verify DNS or a remote TLS connection. Other conditions include `KubernetesResourcesReady`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy`, `S3CredentialsStale`, `Stalled` and `OperationBlocked`.

`DataAtRisk=True` reports Fluss recovery evidence (`dataAtRisk`), even if the cluster is now GREEN, or RED health with hosted replicas and no active leaders. It reports risk; the operator does not restore data. Without server-side recovery evidence, the condition can be `Unknown`.

Fluss 1.0's `getClusterHealth()` supports global counters. The per-pod `assignedTablets` and `replicaHealth` fields require the per-server Admin read API: absent upstream in 1.0, provided by the fork image and observed into status when the server answers; they must remain absent while unavailable, not be invented from Pod readiness.

Kubernetes Pod readiness alone does not prove Fluss health. `RemoteStorageReady=True` carries reference-resolution evidence (referenced Secret with its keys, or ServiceAccount, present) — it does not assert remote operations, and any decision to restart or scale in needs stronger Fluss-specific evidence than a TCP probe.
