# Recreación del lab, reset y registro de versiones

El lab es un clúster k3d más piezas compartidas externas. Nada aquí provisiona credenciales: los secretos S3 se aplican como Secret efímero imperativo en el momento del test, jamás commiteados (ver la discusión de secretos en reposo).

## Recrear desde cero

```bash
# 1. Clúster: 1 server + 3 agents (el placement entre nodos es observable).
k3d cluster create lab --servers 1 --agents 3
kubectl create namespace fluss
kubectl create namespace operator-dev

# 2. ZooKeeper (externo al operador; el operador jamás lo provisiona).
helm repo add bitnami https://charts.bitnami.com/bitnami
helm install zk bitnami/zookeeper \
  --namespace fluss --version 0.15.0

# 3. Fluss de referencia (vía Helm; el operador no toca este namespace).
helm repo add fluss https://downloads.apache.org/fluss/helm-chart
helm install fluss fluss/fluss \
  --namespace fluss --version 1.0.0

# 4. Operador bajo prueba (desde este repo; corre contra operator-dev).
kubectl apply -f deploy/crd.yaml
RUST_LOG=info ./target/debug/nightowl --namespace operator-dev
```
Reaplica `deploy/crd.yaml` tras cualquier cambio en la API (campos nuevos de status o tipos de condición): el apiserver rechaza writes de status con valores de enum desconocidos (422) o poda campos desconocidos en silencio, y el fallo parece que el operador está caído en vez de una CRD rancia.

Espera a `zk-zookeeper-0`, `coordinator-server-0` y los tres `tablet-server-N` en `fluss` antes de probar el operador.

## Acceso desde el host para tests (temporal, retirar después)

Fluss anuncia DNS estables de pod, que el host no resuelve, e IPs de pod, que el host no enruta. Ambas cosas necesitan setup temporal en el host mientras dura el test:

```bash
# Ruta al CIDR de pods vía la IP de cualquier nodo k3d (ver con
# `docker inspect k3d-lab-server-0`).
sudo ip route add 10.42.0.0/16 via <ip-nodo-k3d>

# Una línea de /etc/hosts por pod bajo prueba:
# <ip-pod> <pod>.<headless-svc>.<namespace>.svc.cluster.local
```

Retira ambas al terminar (`ip route del`, borrar las líneas de hosts). Los fallos de test que hablen de resolución de nombres o IPs de pod inalcanzables son fallos del setup del host, no del operador. El Secret S3 efímero se borra con el test (`kubectl delete secret … -n operator-dev`).

## Procedimientos de reset

- **Un test**: borrar el CR, luego todos los PVCs y el Secret efímero en `operator-dev`; parar el operador; retirar ruta y líneas de hosts. El namespace debe quedar vacío (`kubectl get all,pdb,pvc,secrets -n operator-dev` no muestra nada).
- **Lab completo**: `k3d cluster delete lab` y seguir “Recrear desde cero”. Esto borra también el clúster de referencia y la metadata de ZooKeeper —solo hacerlo a propósito.

## Registro de versiones (entorno verificado el 2026-09-27; drill de restore el 2026-09-28)

| Componente | Versión |
| --- | --- |
| k3d | v5.9.0 |
| Kubernetes (k3s) | v1.35.5+k3s1 (1 server + 3 agents) |
| ZooKeeper (chart Bitnami / app) | zookeeper-0.15.0 / 3.9.5 |
| Fluss de referencia (chart / app) | fluss-1.0.0 / `ghcr.io/midnattsol/fluss:1.0.0-midnattsol.3` (plantilla StatefulSet; el rollout de referencia sigue bloqueado) |
| Drill anterior gestionado por el operador | `rst-drill`, imagen `ghcr.io/midnattsol/fluss:1.0.0-midnattsol.4`; se descargó el snapshot, pero falló la recuperación tras perder el disco |
| Drill de restore verificado | `rst-drill5`, imagen `ghcr.io/midnattsol/fluss:1.0.0-midnattsol.5`, construida desde `develop` del fork `d2a1f4382629f8057b8fb0a2cf6b74c90cfb51f3` |
| Drill follower RF=2 | `rst-rf2`, imagen `.5`, prefijo remoto exclusivo; se promocionó el follower reemplazado y recuperó 50/50 filas KV |
| Almacenamiento remoto | RustFS 1.0.0 externo (hardware del lab, fuera de banda) |

Refresca esta tabla cada vez que el lab se mueva. El namespace `fluss` de referencia es una instalación Helm fija para comparar; los tests del operador corren exclusivamente en `operator-dev`.

### Restore tras pérdida de disco RF=1, 2026-09-28

En un clúster nuevo `rst-drill5`, con prefijo S3 exclusivo `clusters/operator-dev/rst-drill5`, Gateway escribió 50 filas KV en `rst5.kv`. Un cliente nativo comprobó **las 50 claves y sus valores de texto exactos**. El snapshot 0 en RustFS registró `row_count=50`, `log_offset=50`, `_METADATA` y `_WRITER_STATE`.

Se borraron el PVC del tabletserver (`a09176b1-b65f-4736-a0ea-f2cae95efbf3`, volumen con el mismo sufijo) y el pod. Kubernetes creó otro PVC (`87bd588b-c734-4196-b6e3-fd95f9a279ec`) y pod; Fluss registró la descarga del snapshot y la recuperación desde offset 50 en el disco nuevo. El cliente volvió a leer **las mismas 50 claves y valores exactos**. Después se escribieron diez filas más y se comprobaron **60/60** claves y valores. El tablet recuperado completó otro snapshot remoto (ID 1).

Fluss informó `GREEN`, líder 1/1 y réplicas 1/1, junto con `data_at_risk=true` persistente; Night Owl fijado a `d2a1f438` informó `DataAtRisk=True` / `SnapshotRecoveryUnverified` aunque hubiera un líder sano. La señal es intencionada: con RF=1 no se puede demostrar que no existieran escrituras reconocidas después del último offset duradero del snapshot o log remoto. Este drill no demuestra RPO cero. Los snapshots hechos con `.4` carecen del checkpoint de escritores necesario para restaurar un log vacío con `.5`; este test creó su snapshot con `.5`.

### Reemplazo y promoción de follower RF=2, 2026-09-28

El clúster aislado `rst-rf2` usó la imagen `.5`, el prefijo S3 exclusivo `clusters/operator-dev/rst-rf2`, dos tabletservers y una tabla `rf2drill.kv` con un bucket y RF=2. El servidor 0 era líder y el 1 follower, con ISR 2/2. Un cliente nativo comprobó 50/50 claves y valores exactos; el snapshot remoto 0 contenía 50 filas en offset 50 y `_WRITER_STATE`.

Se reemplazaron solo el disco y pod del follower: el PVC `95695e67-ee41-4519-abef-15c4dce13d59` pasó a `d9a3ca49-3a44-42c9-878d-7747fbac1c95`, y volvió a ISR 2/2. Tras borrar el **pod** del líder 0 (sin borrar su PVC), el tabletserver 1 asumió el liderazgo en epoch 1. Descargó el snapshot 0 de S3, recuperó KV desde offset 50 y sirvió las 50 claves y valores exactos. Las primeras diez peticiones al Gateway tras el failover devolvieron errores; tras reintentarlas, se escribieron diez claves nuevas y se comprobaron **60/60** valores exactos. El nuevo líder completó el snapshot 1 (`row_count=60`, `log_offset=80`, checkpoint de escritores presente); el offset cuenta actividad WAL, no claves KV distintas. Estado final observado: GREEN, ISR 2/2, `data_at_risk=false`, Night Owl `DataAtRisk=False`. Esto verifica reemplazo y promoción del follower, no la pérdida simultánea de ambas réplicas.

## Listener externo nativo con Envoy, 2026-09-29

Verificado en un k3d separado, `native-access` (un server y dos agents), sin cambiar
el lab de referencia. Envoy Gateway Helm **1.5.0**, `Gateway/v1`,
`TCPRoute/v1alpha2`, imagen Fluss **1.0.0-midnattsol.5** y SDK nativo fijado en
**d2a1f438**. La imagen es anterior al trabajo abandonado de JWT/OAUTHBEARER; no
hicieron falta cambios en el core de Fluss para este acceso externo.

El clúster `native-test/externaltest` tenía dos coordinadores, inicialmente dos
tablets, RF=2 y dos buckets por tabla. ZooKeeper 3.9.3 y RustFS 1.0.0 efímero eran
dependencias del lab. k3s ServiceLB expuso Envoy por la IP del nodo Docker
`172.18.0.2`: coordinadores 23000/23001, tablets 24000/24001 y posteriormente 24002.
No se añadieron rutas al CIDR de pods ni entradas en `/etc/hosts`.

El test desde el host del anterior drill por puertos (sustituido por el diseño SNI):

- Descubrió las direcciones externas del coordinador y los tablets.
- Creó `operator_external.phase1`, escribió 20 filas KV y comprobó sus 20 valores
  exactos a través de Envoy.
- Reemplazó el pod tablet-0 conservando su PVC y volvió a leer 20/20 valores sin
  reescribirlos. El UID cambió de `0438662e-fd6a-41e4-9be0-58e73a66846f` a
  `4d3dff83-d970-49b1-b03a-3cfc5547e94f`.
- Borró el coordinador activo 0. Un cliente nuevo arrancado inmediatamente con
  ambos bootstrap descubrió al coordinador 1 en 23001 y leyó 20/20 valores. Esto
  verifica redescubrimiento del líder, no continuidad de peticiones en una conexión
  ya abierta.
- Escaló de dos a tres tablets y regeneró/aplicó las rutas GitOps. Los metadatos
  incluyeron tablet-2 en 24002; endpoints anteriores e identidades de pods existentes
  permanecieron estables y los 20 valores seguían siendo legibles.

Estado final: GREEN, 4/4 réplicas sincronizadas, 2/2 líderes activos,
`dataAtRisk=false` y cinco mapeos externos en status. Las consultas Admin internas
del operador continuaron funcionando. Esta fase no ejercitó SASL/ACL ni lecturas
cliente de ficheros remotos.

Pruebas adicionales: usar por separado el coordinador activo y cada una de las
tres rutas de tablets como bootstrap permitió leer 20/20 valores. El standby
como único bootstrap devolvió `NotLeader` (código 65); la lista completa de
coordinadores funcionó. Por eso se publican todos los coordinadores, sin depender
de un Service que los balancee aleatoriamente.

Hallazgos de preparación: Envoy reserva 19000/19001 internamente, aunque una ruta
en esos puertos aparezca aceptada. El almacenamiento externo del lab anterior no
era accesible desde la red aislada; las pruebas de datos exitosas utilizaron un
clúster nuevo con RustFS local. Reproducción en
[acceso nativo externo](native-external-access.md).

## Métricas del lab (Prometheus)

### Acceso nativo TLS/SNI con una sola IP, 2026-10-01

Clúster k3d aislado `native-sni` (un server y dos agents, sin tocar el lab de
referencia): Envoy Gateway Helm 1.9.1 con `TLSRoute/v1`, sidecars Envoy 1.33.4,
imagen Fluss `1.0.0-midnattsol.5` y clientes TLS Java/Rust de `feat/clients`.
El DNS `fluss.172.19.0.2.sslip.io` y subdominios `coordinator-N`/`tablet-N`
resolvieron a la misma IP del nodo Docker, puerto 443.

Se generó un certificado de servidor firmado por CA con dominio base y
wildcard. Rust rechazó el primer intento de usar directamente como servidor
un certificado CA autofirmado (`CaUsedAsEndEntity`), confirmando la
validación. Con la cadena corregida, Rust escribió y leyó **20/20** valores
exactos por Envoy passthrough y sidecars; Java leyó **los mismos 20/20**
mediante el cliente incluido en Flink 1.20. Tras borrar Gateway/TLSRoutes
aplicados manualmente, el operador los recreó con owner references y ambos
clientes siguieron leyendo 20/20.

La rotación del Secret a un nuevo certificado firmado por la misma CA cambió
su número de serie sin cambiar el UID del pod tablet (SDS). Con
`internalMapping: true`, el operador creó cinco reglas CoreDNS. Una copia
del fragmento a `coredns-custom` efectuada por la plataforma y un rollout
inicial de CoreDNS hicieron que el dominio base resolviera al ClusterIP de
bootstrap y `tablet-0` a su ClusterIP propio. El operador no hace esa copia.

Tras configurar `security.saslPlain` desde un Secret efímero y habilitar las
ACL nativas, el operador reinició los servidores ordenadamente. Hubo que
dirigir el probe de tablets a INTERNAL: CLIENT ya exigía SASL y el probe no
tenía credenciales. También hubo que registrar el ordinal tras borrarlo en
el secuenciador por claves: antes volvía a borrar el mismo tablet tras Ready.
Ambos defectos quedaron corregidos y probados.

El admin concedió a Alice READ/DESCRIBE sobre `native_sni.phase1` mediante
el cliente nativo Java. **Alice leyó 20/20** valores con Java y Rust por
TLS/SNI + SASL. Bob se autenticó pero recibió una
**`AuthorizationException` al administrar ACLs**, sin poder leer la tabla.
Con el clúster reiniciado, el Admin interno del operador seguía observando
GREEN tras recibir `NotLeader` del coordinador 0 y probar el 1.

Después se reemplazó tablet-0 conservando PVC: cambió el UID y Alice leyó
los 20 valores originales. Se borró el coordinador activo 1: el siguiente
bootstrap descubrió al 0 y volvió a leer 20/20. Al escalar de dos a tres
tablets, el operador creó automáticamente el Service y TLSRoute nuevos; el
cliente descubrió `tablet-2` en la misma IP:443 y leyó 20/20. La plataforma
actualizó el fragmento DNS opcional y reinició CoreDNS: `tablet-2` resolvió
internamente a su propio ClusterIP (`10.43.190.181`), no a la IP de Envoy.
**El operador no ejecuta ese paso de plataforma.** Dos Jobs cliente Java con
el bundle Flink 1.20 leyeron también 20/20 desde Kubernetes: primero por
la IP pública de Envoy y luego con DNS dividido desde dos nodos k3d. El
dominio base resolvió a `native-bootstrap` (`10.43.10.241`) y `tablet-0` a
su ClusterIP (`10.43.6.221`). Son clientes equivalentes a workers, no jobs
Flink. El 2026-10-01 se ejecutó además un clúster Flink 1.20 real: un
JobManager y dos TaskManagers en nodos k3d distintos. Un job SQL batch con el
conector Fluss 1.20 y el **único bootstrap público TLS/SNI** consultó
`SELECT COUNT(*) FROM native_sni.phase1` con SASL y devolvió **20**;
Flink registró el job como `FINISHED` con ambas tareas terminadas. Se ha
verificado la lectura distribuida con Flink, **no** la recuperación de
checkpoints, el failover de Flink, Spark, el acceso desde fuera al almacén de
objetos ni la lectura directa de snapshots remotos por un cliente. El
despliegue Flink y sus credenciales permanecieron fuera de Git. Ver
[acceso nativo externo](native-external-access.md).

Finalmente se redujo de tres tablets a dos solo cuando una lectura Admin
fresca confirmó que tablet-2 estaba registrado y alojaba **cero** réplicas.
El StatefulSet retiró ese pod y el operador eliminó únicamente su TLSRoute
y Service propios. Alice volvió a leer 20/20 valores por el bootstrap base;
el PVC del tablet retirado se conservó.

El 2026-10-01 se aplicaron el CRD y la imagen del operador actualizados al
lab aislado `native-sni`. El perfil RustFS generó acceso por ruta,
`AssumeRole` y STS en el endpoint declarado sin pedir un ARN ficticio al
usuario. Un **usuario IAM** RustFS efímero con política limitada al bucket
sustituyó las claves root en el Secret de Fluss; una sesión STS `AssumeRole`
pudo acceder al bucket. Un NodePort k3d permitió usar el mismo endpoint S3/STS
desde tablets y host. Se cargaron el conector Spark 3.5 y el plugin S3 de
Fluss antes que las clases Hadoop antiguas de Spark (sin este orden aparecía
`NoSuchMethodError` para `Configuration.getEnumSet`). PySpark local leyó el
snapshot KV de `lab_spark.demo`, hizo upsert y releyó **3/3** valores, y los
releyó tras reemplazar un tablet y tras parar/arrancar `native-sni` completo:
**cero errores STS 403**. Los datos perdidos con el bucket efímero anterior
no se recuperaron. Se verificó la red Docker/k3d local, no S3 público ni IAM
AWS.

## Métricas del lab (Prometheus)

Un Prometheus mínimo (`prom/prometheus:v3.5.0`, Deployment + Service en el namespace `monitoring`, manifiestos fuera de este repo) scrapea los pods Fluss por discovery filtrado con las anotaciones `prometheus.io/scrape=true` y `prometheus.io/port=9249`. El reporter se activa por clúster con la API existente —sin cambios en el operador:

```yaml
configurationOverrides:
  metrics.reporters: prometheus
coordinator:
  podTemplate:
    annotations:
      prometheus.io/scrape: "true"
      prometheus.io/port: "9249"
tabletServers:
  podTemplate:
    annotations:
      prometheus.io/scrape: "true"
      prometheus.io/port: "9249"
```

El job (`fluss-pods`) solo conserva targets del puerto 9249 y reescribe `__address__` a IP-de-pod:9249. Verificado el 2026-09-27: 3/3 pods `up`, 1131 series `fluss_*` queryables, gauges del coordinator reflejando el test vivo (`activeTabletServerCount=2`, `tableCount=1`). Tras editar el ConfigMap de scrape, recargar con `POST /-/reload` (el volumen puede tardar ~1 min en sincronizar). Nota: el chart `prometheus` de Bitnami resultó inutilizable (su tag de imagen fijado no resuelve); los manifiestos escritos a mano son el fixture del lab.

## Fluss Gateway (verificado, no desplegado por defecto)

El Gateway upstream **no** viene dentro de `apache/fluss:1.0.0` — es una distribución aparte: contenedor `apache/fluss-gateway:1.0.0`, configurado solo por entorno (`FLUSS_GATEWAY__CLUSTER__DEFAULT__BOOTSTRAP__SERVERS` apuntando a un coordinator gestionado). Verificado el 2026-09-27 contra un clúster del operador: Deployment (imagen stock, una variable) más Service ClusterIP; `/health` y `/ready` OK; creación de tabla de log más 3/3 appends, creación de tabla PK más 2/2 upserts y describe por HTTP plano. Veredicto: uso directo, sin fork ni compilar fuentes. Límites conocidos del preview (1.0): solo modo trust (sin auth/TLS), sin lecturas de registros (lookups/scans piden cliente nativo), writes at-least-once. Fuentes: [Gateway](https://fluss.apache.org/docs/next/gateway), [Deploying](https://fluss.apache.org/docs/install-deploy/deploying-gateway/).

## Forma del workload acotado (writes por Gateway más verify nativo)

El workload reproducible usado para evidencias: un Deployment Gateway efímero junto al clúster de prueba (imagen stock, env de bootstrap, Service ClusterIP, port-forward para acceso host), creación de DB/tablas más batches de filas por REST plano (comprobar `success_count`/`error_count` y `successes`/`failures` por entrada; un HTTP 200 puede traer fallos parciales), y relectura con verificación de integridad por cliente nativo. La verificación debe ser idempotente por conjunto de claves, nunca por conteo físico: el delivery del Gateway es at-least-once y los reintentos pueden duplicar appends de log. Verificado el 2026-09-27: 200 appends de log más 100 upserts PK escritos (300/300 éxitos), 300/300 verificados en nativo (PK por contenido de lookup, log por offsets densos por bucket más bordes de contenido). Truco del lab: el `Debug` de arrow trunca columnas largas (`...80 elements...`), así que jamás parsear el Debug de records para completitud — usar offsets y conjuntos de claves.
