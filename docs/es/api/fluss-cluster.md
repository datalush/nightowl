# FlussCluster

`FlussCluster` es un recurso de Kubernetes con `apiVersion: fluss.datalush.com/v1alpha1`. Su definición está en `src/api.rs`. La CRD se instala para todo Kubernetes, pero el operador crea los recursos de cada clúster en su propio namespace.

> Instala la CRD de la misma versión que el operador. Consulta el [laboratorio local](lab.md) para probar un ejemplo.

## Campos principales de spec

| Campo | Tipo | Obligatorio | Significado |
| --- | --- | --- | --- |
| `version` | cadena | Sí | Versión deseada de Fluss; junto con `image.repository` determina la etiqueta de la imagen. |
| `image` | objeto | Sí | Origen de la imagen y opciones de descarga. |
| `zookeeper` | objeto | Sí | Direcciones de un ZooKeeper gestionado externamente. |
| `coordinator` | objeto | Sí | Réplicas de CoordinatorServer y recursos de los pods. |
| `tabletServers` | objeto | Sí | Réplicas de TabletServer, recursos de los pods y PVC locales. |
| `remoteStorage` | objeto | Sí | Almacenamiento remoto compartido; la API actual ofrece S3. |
| `listeners` | objeto | No | Defaults de INTERNAL/CLIENT y endpoints públicos TLS/SNI opcionales; los CR antiguos no adquieren acceso público. |
| `security.saslPlain` | objeto | No | Secret con usuarios nativos y superusuario ACL, obligatorio al configurar acceso público. |
| `podDisruptionBudget` | objeto | No | Políticas de interrupciones voluntarias para ambos componentes. |
| `rollingUpgrade` | objeto | No | Tiempos para restarts ordenados y conscientes de Fluss (tablets de la cola primero, luego coordinator, con GREEN y estabilización). |
| `scaleIn` | objeto | No | Política de seguridad al retirar un TabletServer. |
| `defaults` | objeto | No | Valores predeterminados para tablas nuevas y mínimo de réplicas sincronizadas. |
| `observability` | objeto | No | Configuración de las métricas Prometheus. |
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
| `coordinator.scheduling` | objeto | No | Distribución de los pods; se explica más abajo. |
| `coordinator.podTemplate` | objeto | No | Etiquetas, anotaciones y contexto de seguridad del pod. |
| `coordinator.configurationOverrides` | mapa de cadenas | No | Propiedades aplicables solo a los Coordinators. |
| `tabletServers.replicas` | entero ≥ 1 | Sí | Procesos TabletServer deseados; no es el factor de replicación de cada tabla. |
| `tabletServers.resources` | objeto | Sí | Reservas de CPU y memoria; límites opcionales. |
| `tabletServers.storage` | objeto | Sí | PVC local por TabletServer. El almacenamiento remoto se configura aparte. |
| `tabletServers.scheduling` | objeto | No | Distribución de los pods; se explica más abajo. |
| `tabletServers.podTemplate` | objeto | No | Etiquetas, anotaciones y contexto de seguridad del pod. |
| `tabletServers.configurationOverrides` | mapa de cadenas | No | Propiedades aplicables solo a los TabletServers. |

Las reservas y límites de CPU y memoria usan cantidades de Kubernetes como `500m` y `2Gi`. Si incluyes `limits`, indica tanto CPU como memoria. Deja margen para memoria fuera del heap: el operador bloquea un `jvm.heap` mayor que la memoria reservada (o el límite, si existe). `storage.size` también usa una cantidad como `20Gi`. `storage.storageClassName` y `storage.dataDir` son opcionales; este último determina la ruta de datos en el pod TabletServer.

Puedes configurar `spreadAcrossNodes`, `nodeSelector`, `affinity`, `tolerations` y `topologySpreadConstraints`. Con `spreadAcrossNodes: true`, el operador añade una regla **preferente** de distribución por nodo (`ScheduleAnyway`); no garantiza que cada réplica quede en un nodo distinto. `podTemplate` contiene metadatos y `securityContext`; las etiquetas del operador prevalecen sobre las del usuario.

El operador amplía los PVC existentes si la StorageClass lo permite, bloquea la reducción de tamaño o el cambio de StorageClass y conserva los PVC al reducir réplicas. Antes de reducir TabletServers, comprueba los servidores que saldrían (ver más abajo).

## Listeners e interrupciones

`listeners.internal` y `listeners.client` usan INTERNAL:9123 y CLIENT:9124 por defecto. Para habilitar TLS/SNI público, configura `listeners.external.domain`, `gateway.className`, `tls.secretName` y `security.saslPlain`. El operador espera a que existan los Secrets TLS y SASL antes de arrancar los pods públicos.

Consulta [acceso nativo externo](native-external-access.md) para configurar DNS y rutas. `status.externalEndpoints` indica que los Services se han creado y `NativeRoutesProgrammed` refleja las condiciones de Gateway API; ninguno demuestra que se pueda conectar desde fuera.

`podDisruptionBudget.tabletServers` exige `enabled` y `maxUnavailable` (entero ≥ 0). Su `coordinator` opcional exige `enabled` y `minAvailable` (entero ≥ 1). Para tablets, `maxUnavailable: 0` bloquea las evacuaciones mediante la API de eviction, pero **no** impide borrar directamente un pod. Si se omite la sección, el operador crea un PDB de TabletServers con `maxUnavailable: 0`; el del Coordinator es opcional. `tabletServers.enabled: false` retira el PDB propio de tablets.

```yaml
listeners:
  internal: { name: INTERNAL, port: 9123 }
  client: { name: CLIENT, port: 9124, serviceType: ClusterIP }
podDisruptionBudget:
  tabletServers: { enabled: true, maxUnavailable: 0 }
  coordinator: { enabled: true, minAvailable: 1 }
```

## Reinicios y reducción de réplicas

`rollingUpgrade` configura tres duraciones: `controlledShutdownTimeout`, `recoveryTimeout` y `stabilizationWindow`. La primera fija el periodo de gracia para apagar el pod (30s si se omite); Fluss gestiona SIGTERM sin un hook preStop. Las duraciones inválidas bloquean la operación.

Los reinicios se hacen pod a pod. Antes de cada borrado se exige una observación reciente de salud GREEN; después se espera a que el pod se recupere y se estabilice. `scaleIn.onNonEmptyTabletServer` solo acepta `Block`: no hay `Force` ni rebalanceo automático.

```yaml
rollingUpgrade:
  controlledShutdownTimeout: 5m
  recoveryTimeout: 30m
  stabilizationWindow: 30s
scaleIn:
  onNonEmptyTabletServer: Block
```

El gate de scale-in consulta la membresía de Fluss y el número actual de réplicas por servidor antes de reducir el StatefulSet. Todos los TabletServers que saldrían deben estar registrados y alojar **cero** réplicas. Si alguno no está vacío o no puede comprobarse, el operador conserva las réplicas existentes e indica el motivo. La lectura por servidor requiere una versión compatible de Fluss: no existe en Fluss 1.0 estándar. El operador no rebalancea tablets automáticamente.

## Valores predeterminados, observabilidad y opciones avanzadas

Si se incluye `defaults`, `tableBuckets` y `logReplicationFactor` son obligatorios y deben ser enteros positivos; `minInSyncReplicas` es opcional:

| Campo | Configuración de Fluss | Alcance |
| --- | --- | --- |
| `defaults.tableBuckets` | `default.bucket.number` | Buckets de **tabla** (sharding) predeterminados para tablas nuevas — sin relación con ningún bucket S3. |
| `defaults.logReplicationFactor` | `default.replication.factor` | Replicación predeterminada del **log** en tablas nuevas; no cuenta pods TabletServer. No debe superar `tabletServers.replicas` (verificado por el esquema; el backstop runtime cubre CRDs instaladas antes de la regla). |
| `defaults.minInSyncReplicas` | `log.replica.min-in-sync-replicas-number` | Mínimo de réplicas sincronizadas para confirmar escrituras del log; ver más abajo. |

Si omites `minInSyncReplicas`, el operador calcula `floor(RF / 2) + 1`
con el factor de replicación efectivo (1 cuando RF=1). Un valor explícito
no puede superar RF. Con RF par, el valor predeterminado prima la durabilidad
sobre la disponibilidad de escritura; coordínalo con la configuración de
confirmaciones del cliente.

### Métricas

Prometheus está activo por defecto: el operador genera `metrics.reporters: prometheus` y añade anotaciones de scrape a los pods de Coordinator y TabletServers. `observability.prometheus: false` lo desactiva. No se crea un Service ni un ServiceMonitor de métricas.

### Configuración adicional

Declara propiedades de Fluss como **cadenas** en `configurationOverrides`, para todo el clúster o por componente. Por ejemplo, `kv.snapshot.interval: "10min"`. Los valores de cada componente prevalecen sobre los globales. No pongas credenciales aquí: usa referencias a Secrets.

El operador rechaza cambios en las claves que gestiona, como listeners, identidad de TabletServer, ruta de ZooKeeper, opciones S3, `data.dir` y valores predeterminados de tablas. El bloqueo identifica la clave en `status`. Las demás claves pasan a Fluss.

### Aplicación de cambios

Fluss 1.0 permite aplicar algunas claves dinámicas, como `kv.snapshot.interval`, mediante Admin sin reiniciar, siempre que Coordinator y TabletServers pidan el mismo valor. `status.appliedDynamicConfig` registra **hashes** de los valores aplicados, nunca los valores originales.

Los demás cambios requieren reinicios secuenciados: los StatefulSets usan `OnDelete` y el operador borra los pods uno a uno tras comprobar la salud. Si el servidor rechaza una clave dinámica, aparece `OperationBlocked` y se intenta un reinicio; los rechazos persistentes siguen visibles sin provocar un bucle de reinicios.

### Gateway HTTP opcional

`gateway.enabled: true` crea un Deployment y un Service ClusterIP. La imagen
predeterminada es `apache/fluss-gateway:<spec.version>` y puede cambiarse.
`status.gateway` muestra las réplicas listas y la URL interna mientras está
activo.

`gateway.ingress` crea además un Ingress con el host y la clase indicados.
El Secret TLS debe existir previamente; si falta, `GatewayBlocked` explica
el bloqueo. La plataforma se encarga de DNS, certificados y autenticación.
Al desactivar el Gateway se retiran solo sus recursos propios.

El Gateway HTTP es incompatible con `security.saslPlain`: no propaga el
principal nativo del llamante y el CRD impide habilitarlos conjuntamente.
El Gateway API con routing SNI nativo es un recurso distinto.

El Ingress admite hosts comodín como `*.example.com` si el controlador Ingress los acepta. El operador solo referencia el Secret TLS; la plataforma debe crearlo.

## Cambios en un clúster existente

Que la CRD acepte un cambio no significa que sea seguro. Los cambios de imagen usan reinicios secuenciados; el scale-in exige la comprobación reciente descrita arriba. Se bloquean las reducciones de PVC y los cambios de StorageClass; la ampliación depende de la StorageClass. No cambies la ruta de ZooKeeper, el bucket ni el prefijo S3 sin migrar los datos. El operador no borra por ti los PVC ni los objetos remotos.

Consulta [almacenamiento remoto y credenciales](remote-storage.md) para los campos S3 y [estado y condiciones](status.md) para la información observada.
