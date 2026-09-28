# Auditoría FIP-41 y matriz de capacidades Admin

FIP-41 ([wiki](https://cwiki.apache.org/confluence/spaces/FLUSS/pages/421957775/FIP-41+Fluss+Kubernetes+Operator), estado **accepted**, paraguas [apache/fluss#3787](https://github.com/apache/fluss/issues/3787)) propone un operador Java (JOSDK) desde un repositorio dedicado. Este operador es deliberadamente distinto: **Rust** (`kube-rs` + `fluss-rs`), desarrollado en este repositorio. La decisión de viabilidad de Rust es afirmativa —el bucle completo de reconciliación (bootstrap, config, almacenamiento, PDBs, salud, condiciones) está verificado en vivo en k3d en las pruebas recogidas en estos docs. El lenguaje no elimina el único hueco de servidor que importa (abajo).

Convenciones: **soportado** (implementado y verificado en lab), **bloqueado** (pide trabajo upstream; mientras tanto fail-closed), **deliberadamente distinto** (divergencia consciente con motivo), **diferido** (seguido en otro sitio, sin empezar).

## Matriz de requisitos

| Requisito FIP-41 | Veredicto | Notas |
| --- | --- | --- |
| Implementación Java/JOSDK | Deliberadamente distinto | Operador Rust; viabilidad probada con pruebas verificadas. |
| Grupo CRD `fluss.apache.org/v1alpha1` | Deliberadamente distinto | El nuestro es un grupo distinto para no colisionar con una futura CRD oficial. |
| Réplicas del coordinator = 1 | Soportado | Un solo coordinator, forzado. |
| `tablet-server.id` derivado del ordinal, vínculo estable id↔PVC↔datos | Soportado | Verificado ante reinicios y rollouts. |
| PVCs vía `volumeClaimTemplates`, Retain/Retain, jamás auto-borrar PVCs/S3/ZK | Soportado | Verificado; shrink y cambio de StorageClass rechazados. |
| Orden de arranque: config → coordinator → tablets → Ready | Soportado | Verificado, incluida idempotencia ante reinicios. |
| PDB `maxUnavailable: 0` en tablets; el operador rota por borrado directo | Soportado | Verificado; el PDB solo frena evictions, no el borrado directo. |
| Gate de scale-in (rechazar servidor no vacío) | Soportado (parcial) | El gate consulta lectura fresca por servidor (imagen fork): registrado-y-vacío converge, lo demás falla cerrado con el motivo exacto. Workflow vivo de borrado pendiente en `g8qz`. |
| Rolling upgrade: tablets primero, gate `serverGreen` por servidor, estado `Stalled` | Bloqueado | Los restarts secuenciados corren (tablets de la cola primero, con GREEN, `Stalled` con evidencia); la orquestación de upgrade por imagen sigue pendiente (`cm74`). |
| Clasificación de config dinámica vs con restart | Soportado (parcial) | Propiedad de claves forzada, claves desconocidas fallan cerrado; aplicar dinámicas vía Admin está pendiente. |
| Hash de la config renderizada dirigiendo restarts | Soportado | El hash de config en el pod template dirige al secuenciador de restarts (StatefulSets en OnDelete; sin rolling nativo de Kubernetes). |
| Resize de almacenamiento (expandir in place, orphan-recreate para la plantilla) | Soportado (parcial) | Expansión verificada en vivo; shrink rechazado. El path orphan-recreate está aceptado pero sin orquestar. |
| Ciclo de vida solo con condiciones (`Ready`, `Progressing`, `Upgrading`, `Stalled`, `Degraded`) | Deliberadamente distinto | Condiciones por área (`KubernetesResourcesReady`, `RemoteStorageReady`, `FlussReachable`, `ClusterHealthy`, `S3CredentialsStale`) más `Stalled` para stalls de restarts secuenciados; el resto pertenece al trabajo de upgrade. |
| `server.yaml` renderizado en un Secret | Deliberadamente distinto | La config renderizada es un ConfigMap con **marcadores** (`${directory:…}`), nunca valores de credenciales; los valores quedan en el Secret montado. Estrictamente más fuerte que el Secret-con-valores de FIP-41. |
| Listeners estructurados + derivación de DNS anunciado | Soportado | Los listeners interno/cliente dirigen Services y direcciones anunciadas. |
| El operador jamás dispara `rebalance()` en v1alpha1 | Soportado | El operador nunca llama a rebalance; la evacuación sigue siendo manual. |
| Validación CEL/admission en vez de webhook opcional | Deliberadamente distinto | Reglas de esquema en la CRD; sin webhook que operar. |
| Watch de todo el clúster por defecto, RBAC mínimo | Soportado | Verificado con ServiceAccount sin privilegios; aislamiento entre namespaces probado. |
| ZooKeeper externo; sin gestión de ZK | Soportado | ZK es configuración, jamás se provisiona. |
| Defaults de tabla (buckets, RF, min-ISR) | Deliberadamente distinto (añadido) | FIP-41 no tiene sección de defaults; el nuestro renderiza `default.bucket.number` y guardas de replicación. |
| Almacenamiento S3 estructurado + delegación | Deliberadamente distinto (añadido) | Más allá de la limitación de claves en texto plano de FIP-41; marcadores de Secret más AssumeRole/GetSessionToken verificados. |
| Leader election del operador vía Lease | Diferido | Una sola réplica hoy; seguido con el empaquetado. |
| Endpoint de métricas Prometheus | Soportado (parcial) | Clave de reporter más anotaciones de scrape cableadas, activas por defecto con opt-out; Service/ServiceMonitor de métricas y métricas propias del reconciler siguen diferidos con el empaquetado. |
| Adopción in-place (Path A) / reemplazo con drain (Path B) | Diferido | Seguido por separado; el operador jamás adopta recursos ajenos. |
| Job de lake tiering, gestión de tablas, orquestación de backup/restore | Fuera de alcance | Coincide con los non-goals de FIP-41; restore es observar e informar. |

## Respuestas directas que pide la auditoría

- **`describeTabletServers`**: no existe en Fluss 1.0; implementado en nuestra imagen del fork (`1.0.0-midnattsol.1`, ApiKey 1067) y observado en vivo en el clúster el 2026-09-27 (3/3 servidores con contadores, clúster GREEN). Las peticiones upstream siguen abiertas ([apache/fluss#3743](https://github.com/apache/fluss/issues/3743), [apache/fluss#3570](https://github.com/apache/fluss/issues/3570)). El scale-in de servidores no vacíos y el gate de upgrade por servidor siguen rechazados hasta cablearse.
- **`listServerTags`**: no existe (la propia FIP-41 dice que los tags se añaden/quitan pero no se listan). Ningún flujo del operador depende aún de ello.
- **Señales de salud para upgrade**: `getClusterHealth()` de clúster existe y se usa (GREEN/YELLOW/RED/UNKNOWN dirige la condición `ClusterHealthy`). La salud por servidor no existe —el mismo hueco de arriba. No hay auto-rollback en ningún sitio, por diseño.
- **Drain mode / `decommissionServer` / rebalance min-ISR-aware**: nada existe en el servidor; todo diferido con la adopción.

## Matriz de capacidades Admin (`fluss-rs` 1.0.0 vs servidor Fluss 1.0)

| Capacidad | Servidor 1.0 | `fluss-rs` 1.0 | Uso del operador | Veredicto |
| --- | --- | --- | --- | --- |
| `getClusterHealth` | Sí | `get_cluster_health` | Condición `ClusterHealthy` | Soportado |
| `describeTabletServers` (réplicas/ISR/líderes por servidor) | No (stock) / Sí (imagen fork) | Sí (pin fork) | Observado en el status; gate de scale-in y `serverGreen` pendientes | Parcial |
| `listServerTags` | No | No | Acotar rebalance (v1beta1) | Bloqueado |
| `addServerTag` / `removeServerTag` | Sí | Sí | Sin usar (jamás dirigir rebalance) | Disponible |
| `rebalance` / `listRebalanceProgress` / `cancelRebalance` | Sí | Sí | Deliberadamente sin usar | Deliberadamente distinto |
| `describeClusterConfigs` / `alterClusterConfigs` | Sí | Sí | Propiedad forzada; aplicar dinámicas pendiente | Parcial |
| Lectura de snapshots KV + leases (`getLatestKvSnapshots`, metadata, adquirir/liberar/borrar lease) | Sí | Sí | Verificación en lab; el operador jamás mueve bytes | Soportado |
| `GetFileSystemSecurityToken` + renovación en cliente | Sí | `SecurityTokenManager` | Flujo de tokens verificado | Soportado |
| Gestión de tablas/bases/particiones/ACLs | Sí | Sí | Solo tooling de lab; sin CRD de tablas | Deliberadamente distinto |
| `getServerNodes` | Sí | Sí | Bootstrap/metadata | Disponible |
| Offsets, snapshots lake, manifests de remote-log, producer offsets | Sí | Sí | Sin usar | Disponible |

Fuentes: [wiki FIP-41](https://cwiki.apache.org/confluence/spaces/FLUSS/pages/421957775/FIP-41+Fluss+Kubernetes+Operator), [issue paraguas](https://github.com/apache/fluss/issues/3787), [`DescribeTabletServers` upstream](https://github.com/apache/fluss/issues/3743), [`fluss-rs` 1.0.0 `admin.rs`](https://docs.rs/fluss), [configuración S3 de Fluss 1.0](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/).
