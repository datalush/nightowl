# FlussCluster

`FlussCluster` es un recurso personalizado de Kubernetes limitado a un namespace, con `apiVersion: fluss.datalush.com/v1alpha1`. Su definición canónica está en `operator/src/api.rs`. La CRD se registra para todo el clúster de Kubernetes, pero cada instancia pertenece a un namespace. En el futuro, el controlador creará los recursos de cada instancia en ese mismo namespace y derivará sus nombres de `metadata.name`.

> **Contrato frente a controlador:** ya existen los tipos Rust y el esquema. El programa actual no crea cargas de trabajo ni aplica las reglas operativas descritas aquí. Consulta el [estado actual](../current-state.md).

## Campos principales de spec

| Campo | Tipo | Obligatorio | Significado |
| --- | --- | --- | --- |
| `version` | cadena | Sí | Versión deseada de Fluss; junto con `image.repository` determina la etiqueta de la imagen. |
| `image` | objeto | Sí | Origen de la imagen y opciones de descarga. |
| `zookeeper` | objeto | Sí | Direcciones de un ZooKeeper gestionado externamente. |
| `coordinator` | objeto | Sí | Réplicas de CoordinatorServer y recursos de los pods. |
| `tabletServers` | objeto | Sí | Réplicas de TabletServer, recursos de los pods y PVC locales. |
| `remoteStorage` | objeto | Sí | Almacenamiento remoto compartido; la API actual ofrece S3. |
| `listeners` | objeto | No | Nombres y puertos de los listeners interno y cliente; acceso interno al clúster. |
| `podDisruptionBudget` | objeto | No | Políticas de interrupciones voluntarias para ambos componentes. |
| `rollingUpgrade` | objeto | No | Tiempos para una futura actualización ordenada y consciente de Fluss. |
| `scaleIn` | objeto | No | Política de seguridad al retirar un TabletServer. |
| `defaults` | objeto | No | Valores predeterminados para tablas nuevas y mínimo de réplicas sincronizadas. |
| `observability` | objeto | No | Intención de habilitar el reporter Prometheus. |
| `configurationOverrides` | mapa de cadenas | No | Propiedades adicionales de `server.yaml` de Fluss. |

Omitir un campo opcional **no** implica que el Operador ya haya elegido un valor apropiado para producción. La validación del esquema cubre tipos y algunos límites numéricos; no sustituye la validación del despliegue.

## Imagen y ZooKeeper

| Campo | Tipo | Obligatorio | Notas |
| --- | --- | --- | --- |
| `image.repository` | cadena | Sí | Por ejemplo, `apache/fluss`. La etiqueta procede de `spec.version`. |
| `image.pullPolicy` | `Always`, `IfNotPresent`, `Never` | No | Puede utilizar el valor predeterminado del workload de Kubernetes. |
| `image.pullSecrets` | lista de nombres | No | Referencias a Secrets del **mismo namespace** que `FlussCluster`. |
| `zookeeper.addresses` | lista no vacía de cadenas | Sí | Fluss recibe `zookeeper.address` separado por comas; cada entrada debería ser un `host:puerto` accesible. |
| `zookeeper.pathRoot` | cadena | No | Por defecto `/fluss/<namespace>/<nombre>` si se omite. |

Varios Coordinators con el mismo path de ZooKeeper participan en la elección de líder de Fluss 1.0. Un único pod ZooKeeper sigue siendo un punto único de fallo. Tanto la ruta de ZooKeeper como la ubicación remota identifican datos existentes: deben mantenerse estables durante reinicios y futuras actualizaciones.

## Coordinator y TabletServers

| Campo | Tipo | Obligatorio | Notas |
| --- | --- | --- | --- |
| `coordinator.replicas` | entero ≥ 1 | Sí | Fluss 1.0 admite varios CoordinatorServers: uno es líder y los demás quedan en espera. |
| `coordinator.resources` | objeto | Sí | Reservas de CPU y memoria; límites opcionales. |
| `coordinator.image`, `tabletServers.image` | cadena | No | Imagen completa por componente; comprobar su compatibilidad con la versión antes de actualizar. |
| `coordinator.jvm`, `tabletServers.jvm` | objeto | No | `heap` obligatorio si se incluye el objeto y `extraArgs` opcional. |
| `coordinator.storage` | objeto | No | PVC local opcional. ZooKeeper sigue almacenando los metadatos. |
| `coordinator.scheduling` | objeto | No | Intención de ubicación; se explica más abajo. |
| `coordinator.podTemplate` | objeto | No | Etiquetas, anotaciones y contexto de seguridad del pod. |
| `coordinator.configurationOverrides` | mapa de cadenas | No | Propiedades aplicables solo a los Coordinators. |
| `tabletServers.replicas` | entero ≥ 1 | Sí | Procesos TabletServer deseados; no es el factor de replicación de cada tabla. |
| `tabletServers.resources` | objeto | Sí | Reservas de CPU y memoria; límites opcionales. |
| `tabletServers.storage` | objeto | Sí | PVC local por TabletServer. El almacenamiento remoto se configura aparte. |
| `tabletServers.scheduling` | objeto | No | Intención de ubicación; se explica más abajo. |
| `tabletServers.podTemplate` | objeto | No | Etiquetas, anotaciones y contexto de seguridad del pod. |
| `tabletServers.configurationOverrides` | mapa de cadenas | No | Propiedades aplicables solo a los TabletServers. |

`resources.requests.cpu`, `resources.requests.memory`, `resources.limits.cpu` y `resources.limits.memory` son cadenas con cantidades de Kubernetes, como `500m` y `2Gi`. Si se incluye `limits`, el tipo Rust actual exige **CPU y memoria**. `jvm.heap` es una cadena como `1Gi`; el futuro controlador deberá comprobar que el límite de memoria deja margen para memoria fuera del heap. `storage.size` es una cadena como `20Gi`; `storage.storageClassName` y `storage.dataDir` son opcionales. `tabletServers.storage.dataDir` indica dónde montar los datos dentro del pod TabletServer.

La ubicación admite `spreadAcrossNodes` (booleano), `nodeSelector` (mapa de cadenas) y los tipos nativos de Kubernetes `affinity`, `tolerations` y `topologySpreadConstraints`. Todos son opcionales. `podTemplate` contiene **solo** metadatos y `securityContext`: no existe otra afinidad o selector de nodos que pueda contradecir `scheduling`. Las etiquetas del usuario no deben sustituir las etiquetas de propiedad o selección de Services del Operador. El futuro controlador deberá convertir `spreadAcrossNodes` en reglas reales; ponerlo en un CR hoy no distribuye nada. La expansión de StorageClasses, la inmutabilidad, la retención de PVC y la reducción de servidores necesitan un ciclo de vida explícito.

## Listeners e interrupciones

`listeners.internal` exige `name` y `port`. `listeners.client` exige `name`, `port` y `serviceType`; esta versión de la API solo admite `ClusterIP`. Si se configura `listeners`, ambos objetos son obligatorios. Los puertos deben estar entre 1 y 65535; los nombres deberían ser distintos. El futuro controlador deberá generar Services y configuración del servidor a partir de **los mismos valores** y derivar `advertised.listeners` del DNS de cada pod. El acceso de clientes externos aún no está modelado.

`podDisruptionBudget.tabletServers` exige `enabled` y `maxUnavailable` (entero ≥ 0). Su `coordinator` opcional exige `enabled` y `minAvailable` (entero ≥ 1). Para tablets, `maxUnavailable: 0` bloquea las evacuaciones mediante la API de eviction, pero **no** impide borrar directamente un pod. Si se omite `podDisruptionBudget`, el esquema aún no establece ningún valor por defecto. El futuro controlador debería usar `maxUnavailable: 0` por defecto para TabletServers.

```yaml
listeners:
  internal: { name: INTERNAL, port: 9123 }
  client: { name: CLIENT, port: 9124, serviceType: ClusterIP }
podDisruptionBudget:
  tabletServers: { enabled: true, maxUnavailable: 0 }
  coordinator: { enabled: true, minAvailable: 1 }
```

## Intención de ciclo de vida

`rollingUpgrade` recibe tres cadenas de duración obligatorias: `controlledShutdownTimeout` para la salida controlada, `recoveryTimeout` para recuperar el pod y `stabilizationWindow` antes de pasar al siguiente. El esquema Rust aún no valida el formato ni coordina actualizaciones. `scaleIn.onNonEmptyTabletServer` solo admite **`Block`**: no reducir el StatefulSet si el TabletServer que saldría sigue alojando réplicas. Esta API no tiene `Force` ni rebalanceo automático.

```yaml
rollingUpgrade:
  controlledShutdownTimeout: 5m
  recoveryTimeout: 30m
  stabilizationWindow: 30s
scaleIn:
  onNonEmptyTabletServer: Block
```

`Block` es una política declarada, **todavía no una comprobación implementada**. Para hacerla cumplir se necesita la API propuesta de Fluss que consulta el número de réplicas por servidor. Hasta que el Operator pueda demostrar que una reducción o actualización es segura, deberá conservar los recursos en ejecución e indicar el bloqueo en `status`.

## Valores predeterminados, observabilidad y opciones avanzadas

Si se incluye `defaults`, `tableBuckets` y `logReplicationFactor` son obligatorios y deben ser enteros positivos; `minInSyncReplicas` es opcional:

| Campo | Configuración de Fluss | Alcance |
| --- | --- | --- |
| `defaults.tableBuckets` | `default.bucket.number` | Buckets de **tabla** (sharding) predeterminados para tablas nuevas — sin relación con ningún bucket S3. |
| `defaults.logReplicationFactor` | `default.replication.factor` | Replicación predeterminada del **log** en tablas nuevas; no cuenta pods TabletServer. No debe superar `tabletServers.replicas` (verificado por el esquema; el backstop runtime cubre CRDs instaladas antes de la regla). |
| `defaults.minInSyncReplicas` | `log.replica.min-in-sync-replicas-number` | Durabilidad de escritura de log a nivel de servidor con `acks=all` (no es un predeterminado por tabla). Si se omite, el Operador genera el quorum `floor(RF / 2) + 1` a partir del factor de replicación **efectivo** tras fusionar overrides (1 cuando RF es 1); un valor explícito no debe superarlo. Con factores de replicación pares esto prima durabilidad sobre disponibilidad —documentado, no prohibido. Afecta a escrituras que esperan confirmación de todas las réplicas; coordinar con los ajustes de confirmación del cliente. |

`observability.prometheus` es booleano y toma `false` como valor predeterminado en el tipo Rust serializado. El endpoint y el Service de Prometheus todavía no se reconcilian. `configurationOverrides` puede establecerse globalmente y por componente, con nombres de propiedades Fluss y valores de tipo **cadena**, por ejemplo `kv.snapshot.interval: "10min"`. Los valores específicos del componente prevalecen sobre los globales. No pongas credenciales en ninguno de ellos. El reconciler **aplica** la propiedad de claves: las de identidad, topología, credenciales, cableado de almacenamiento y defaults renderizados (listeners, identidad de TabletServer, ruta ZooKeeper, propiedades S3, `data.dir`, defaults de tabla) se rechazan con estado `ConfigBlocked` que nombra la clave; las de afinado (`kv.*`, `netty.*`, …) y las futuras desconocidas pasan. Los cambios dinámicos y los que exigen reinicio necesitarán flujos distintos.

## Cambios en un clúster existente

El esquema permite modificar estos campos, pero eso **no** significa que las operaciones sean seguras. Cambiar la versión, reducir TabletServers o PVC, o mover los datos de ZooKeeper/S3 no debe convertirse en un parche directo al StatefulSet. Hasta implementar el ciclo de vida correspondiente, esas peticiones deberán rechazarse o bloquearse con una condición clara. Ni los volúmenes existentes ni los objetos remotos deben eliminarse automáticamente.

Consulta [almacenamiento remoto y credenciales](remote-storage.md) para los campos S3 y [estado y condiciones](status.md) para la información observada.
