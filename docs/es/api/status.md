# Estado y condiciones

`status` describe lo observado. **No** lo establece quien crea un `FlussCluster`. Los campos quedan ausentes hasta que el controlador puede observarlos de verdad; ausente es honesto, inventado no.

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

Es un **ejemplo de estructura**; los endpoints y contadores son ilustrativos. `observedVersion` debe basarse en el clúster real, no copiarse de `spec.version`.

## Salud del lado Fluss

Cuando el coordinator responde por el listener interno, el controlador rellena además `clusterHealth` (contadores globales de réplicas, ISR y líderes desde `getClusterHealth`), `coordinatorEndpoints` (coordinators vistos en membership), `coordinator` (deseadas frente a registradas) y `tabletServers` (deseadas frente a miembros registrados, una entrada por servidor), más las condiciones `FlussReachable` y `ClusterHealthy`. La membresía la observa Fluss — estrictamente más honesto que el Ready de pods para saber "cuántos servidores sirven".

Las sondas corren como mucho cada 60 segundos por clúster (límite en memoria, sin churn en `.status`); entre sondas valen los últimos valores observados. Un clúster inalcanzable reporta `FlussReachable=False` con la causa y deja `ClusterHealthy` en su valor previo — o ausente si nunca se observó. La observación de salud nunca bloquea la convergencia ni reintenta en caliente.

`assignedTablets` y `replicaHealth` por pod se rellenan cuando el servidor responde `DescribeTabletServers` (imagen del fork `1.0.0-midnattsol.1` en adelante, verificado en vivo 2026-09-27); `coordinator.activePod` sigue ausente. Contra Fluss 1.0 stock quedan ausentes mientras no exista, sin deducirse de que el pod esté listo.

| Campo | Tipo | Significado |
| --- | --- | --- |
| `observedGeneration` | entero opcional | Última generación del CR cuyo estado deseado se ha procesado. |
| `observedConfigHash` | cadena opcional | `sha256:<hex>` combinado de los documentos `server.yaml` de coordinator y tablet (coordinator primero); cada ConfigMap lleva además su propio hash en anotación para futuros rollouts. |
| `observedVersion` | cadena opcional | Versión confirmada mediante observación, no simplemente solicitada. |
| `appliedDynamicConfig` | mapa opcional | Claves dinámicas aplicadas a hashes de valor (solo hashes, nunca valores); registra lo que Admin ya contiene. |
| `clusterHealth` | objeto opcional | Estado `GREEN`, `YELLOW`, `RED` o `UNKNOWN` y contadores globales de réplicas, ISR y líderes. |
| `coordinatorEndpoints` | lista de cadenas | Endpoints de Coordinator para los clientes. |
| `coordinator` | objeto opcional | Réplicas deseadas/listas y `activePod` opcional. |
| `tabletServers` | objeto opcional | Réplicas deseadas/listas y, por pod, `assignedTablets` y `replicaHealth` opcionales. |
| `gateway` | objeto opcional | Réplicas deseadas/listas del Gateway más la URL interna; ausente si no se pide. |
| `conditions` | lista | Indicadores operativos independientes con evidencia y momento de transición. |

Cada condición contiene `type`, `status`, `reason`, `message`, `evidence` y `lastTransitionTime`. El esquema limita `status` a `"True"`, `"False"` y `"Unknown"`, y `type` a `Ready`, `Progressing`, `Upgrading`, `Stalled`, `Degraded`, `Adoptable`, `KubernetesResourcesReady`, `ZooKeeperReachable`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy`, `S3CredentialsStale` y `OperationBlocked`. Hoy corren `KubernetesResourcesReady`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy`, `S3CredentialsStale`, `Stalled` (stalls de restarts secuenciados) y `OperationBlocked` (rechazos de config dinámica); el resto están planificadas.

`getClusterHealth()` de Fluss 1.0 proporciona contadores globales. `assignedTablets` y `replicaHealth` por pod requieren la API Admin de lectura por servidor: ausente upstream en 1.0, provista por la imagen del fork y observada en el status cuando el servidor responde; deben quedar ausentes mientras no exista, no deducirse de que el pod esté listo.

Un pod listo en Kubernetes no demuestra que Fluss esté saludable. `RemoteStorageReady=True` lleva evidencia de resolución de referencias (Secret referenciado con sus claves, o ServiceAccount, presentes) —no demuestra operaciones remotas; reiniciar o reducir servidores requiere algo más que una sonda TCP.
