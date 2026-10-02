# Estado y condiciones

El operador escribe `status` a partir de lo que observa; no forma parte de la configuración que aplicas. Los campos que no puede comprobar quedan ausentes.

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
      lastTransitionTime: "<instante de la transición>"
```

Es un fragmento, no un `status` completo; los nombres y contadores son ilustrativos. `observedVersion` no se copia sin más de `spec.version`.

## Salud del lado Fluss

Cuando el Coordinator responde por el listener interno, el operador registra
en `clusterHealth` los contadores globales de réplicas y líderes. También
informa de los endpoints de Coordinator y de los servidores registrados en
`coordinator` y `tabletServers`. Las condiciones `FlussReachable` y
`ClusterHealthy` reflejan esta consulta. Para saber cuántos servidores están
disponibles, importa la membresía de Fluss, no solo si Kubernetes marca sus
pods como listos.

Las sondas corren como mucho cada 60 segundos por clúster (límite en memoria, sin churn en `.status`); entre sondas valen los últimos valores observados. Un clúster inalcanzable reporta `FlussReachable=False` con la causa y deja `ClusterHealthy` en su valor previo — o ausente si nunca se observó. La observación de salud nunca bloquea la convergencia ni reintenta en caliente.

`assignedTablets` y `replicaHealth` por pod se rellenan si el servidor responde `DescribeTabletServers` (disponible en la imagen compatible del fork, no en Fluss 1.0 estándar). `coordinator.activePod` queda ausente; el operador no lo deduce de que el pod esté listo.

| Campo | Tipo | Significado |
| --- | --- | --- |
| `observedGeneration` | entero opcional | Última generación del CR cuyo estado deseado se ha procesado. |
| `observedConfigHash` | cadena opcional | Hash combinado de la configuración de coordinator y tablet, incluida la identidad externa generada cuando existe. |
| `observedVersion` | cadena opcional | Versión confirmada mediante observación, no simplemente solicitada. |
| `appliedDynamicConfig` | mapa opcional | Claves dinámicas aplicadas a hashes de valor (solo hashes, nunca valores); registra lo que Admin ya contiene. |
| `clusterHealth` | objeto opcional | Estado `GREEN`, `YELLOW`, `RED` o `UNKNOWN` y contadores globales de réplicas, ISR y líderes. |
| `coordinatorEndpoints` | lista de cadenas | Endpoints internos observados; los clientes externos usan el bootstrap público. |
| `externalEndpoints` | lista de objetos | Direcciones públicas cuyos Services propios convergieron; no prueba conectividad. |
| `coordinator` | objeto opcional | Réplicas deseadas/listas y `activePod` opcional. |
| `tabletServers` | objeto opcional | Réplicas deseadas/listas y, por pod, `assignedTablets` y `replicaHealth` opcionales. |
| `gateway` | objeto opcional | Réplicas deseadas/listas del Gateway más la URL interna; ausente si no se pide. |
| `conditions` | lista | Indicadores operativos independientes con evidencia y momento de transición. |

Cada condición contiene `type`, `status`, `reason`, `message`, `evidence` y `lastTransitionTime`. `status` acepta `"True"`, `"False"` y `"Unknown"`.

`NativeRoutesProgrammed=True` indica que Gateway y TLSRoutes informan de rutas programadas y aceptadas; no prueba DNS ni una conexión TLS desde fuera. También se emiten `KubernetesResourcesReady`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy`, `S3CredentialsStale`, `Stalled` y `OperationBlocked`.

`DataAtRisk=True` indica que Fluss ha informado de riesgo durante la recuperación (`dataAtRisk`), incluso si ahora está GREEN, o que hay réplicas alojadas sin líderes activos con salud RED. El operador informa del riesgo, pero no restaura datos. Sin evidencia de recuperación del servidor, la condición puede ser `Unknown`.

`getClusterHealth()` de Fluss 1.0 proporciona contadores globales. `assignedTablets` y `replicaHealth` por pod requieren la API Admin de lectura por servidor: ausente upstream en 1.0, provista por la imagen del fork y observada en el status cuando el servidor responde; deben quedar ausentes mientras no exista, no deducirse de que el pod esté listo.

Un pod listo en Kubernetes no demuestra que Fluss esté saludable. `RemoteStorageReady=True` lleva evidencia de resolución de referencias (Secret referenciado con sus claves, o ServiceAccount, presentes) —no demuestra operaciones remotas; reiniciar o reducir servidores requiere algo más que una sonda TCP.
