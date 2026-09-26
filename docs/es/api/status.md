# Estado y condiciones

`status` describe lo observado. **No** lo establece quien crea un `FlussCluster`. La API Rust define la siguiente estructura, pero el watcher actual no actualiza el subrecurso de estado.

```yaml
status:
  observedGeneration: 3
  observedConfigHash: "sha256:ejemplo"
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
      message: Un CoordinatorServer respondió a una consulta administrativa de Fluss.
      evidence:
        - "2 CoordinatorServers configurados"
      lastTransitionTime: "2026-09-25T12:00:00Z"
```

Es un **ejemplo de estructura**, no una respuesta producida por el Operador actual. Los endpoints y contadores son ilustrativos; solo se muestra uno de los tres pods para no alargar el ejemplo. `observedVersion` debe basarse en el clúster real, no copiarse de `spec.version`.

| Campo | Tipo | Significado |
| --- | --- | --- |
| `observedGeneration` | entero opcional | Última generación del CR cuyo estado deseado se ha procesado. |
| `observedConfigHash` | cadena opcional | Hash de la configuración generada y observada. |
| `observedVersion` | cadena opcional | Versión confirmada mediante observación, no simplemente solicitada. |
| `clusterHealth` | objeto opcional | Estado `GREEN`, `YELLOW`, `RED` o `UNKNOWN` y contadores globales de réplicas, ISR y líderes. |
| `coordinatorEndpoints` | lista de cadenas | Endpoints de Coordinator para los clientes. |
| `coordinator` | objeto opcional | Réplicas deseadas/listas y `activePod` opcional. |
| `tabletServers` | objeto opcional | Réplicas deseadas/listas y, por pod, `assignedTablets` y `replicaHealth` opcionales. |
| `conditions` | lista | Indicadores operativos independientes con evidencia y momento de transición. |

Cada condición contiene `type`, `status`, `reason`, `message`, `evidence` y `lastTransitionTime`. El esquema limita `status` a `"True"`, `"False"` y `"Unknown"`, y `type` a `Ready`, `Progressing`, `Upgrading`, `Stalled`, `Degraded`, `Adoptable`, `KubernetesResourcesReady`, `ZooKeeperReachable`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy` y `OperationBlocked`. Estas comprobaciones son **comportamiento propuesto**; todavía no se ejecutan. `lastTransitionTime` sigue siendo una cadena cuyo formato y semántica quedan por implementar.

`getClusterHealth()` de Fluss 1.0 proporciona contadores globales. `assignedTablets` y `replicaHealth` por pod requieren la API Admin de lectura por servidor propuesta por FIP-41: deben quedar ausentes mientras no exista, no deducirse de que el pod esté listo.

Un pod listo en Kubernetes no demuestra que Fluss esté saludable. `RemoteStorageReady=True` lleva evidencia de resolución de referencias (Secret referenciado con sus claves, o ServiceAccount, presentes) —no demuestra operaciones remotas; reiniciar o reducir servidores requiere algo más que una sonda TCP.
