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
| Almacenamiento remoto | RustFS 1.0.0 externo (hardware del lab, fuera de banda) |

Refresca esta tabla cada vez que el lab se mueva. El namespace `fluss` de referencia es una instalación Helm fija para comparar; los tests del operador corren exclusivamente en `operator-dev`.

### Restore tras pérdida de disco RF=1, 2026-09-28

En un clúster nuevo `rst-drill5`, con prefijo S3 exclusivo `clusters/operator-dev/rst-drill5`, Gateway escribió 50 filas KV en `rst5.kv`. Un cliente nativo comprobó **las 50 claves y sus valores de texto exactos**. El snapshot 0 en RustFS registró `row_count=50`, `log_offset=50`, `_METADATA` y `_WRITER_STATE`.

Se borraron el PVC del tabletserver (`a09176b1-b65f-4736-a0ea-f2cae95efbf3`, volumen con el mismo sufijo) y el pod. Kubernetes creó otro PVC (`87bd588b-c734-4196-b6e3-fd95f9a279ec`) y pod; Fluss registró la descarga del snapshot y la recuperación desde offset 50 en el disco nuevo. El cliente volvió a leer **las mismas 50 claves y valores exactos**. Después se escribieron diez filas más y se comprobaron **60/60** claves y valores. El tablet recuperado completó otro snapshot remoto (ID 1).

Fluss informó `GREEN`, líder 1/1 y réplicas 1/1, junto con `data_at_risk=true` persistente; Night Owl fijado a `d2a1f438` informó `DataAtRisk=True` / `SnapshotRecoveryUnverified` aunque hubiera un líder sano. La señal es intencionada: con RF=1 no se puede demostrar que no existieran escrituras reconocidas después del último offset duradero del snapshot o log remoto. Este drill no demuestra RPO cero. Los snapshots hechos con `.4` carecen del checkpoint de escritores necesario para restaurar un log vacío con `.5`; este test creó su snapshot con `.5`.

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
