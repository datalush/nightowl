# Laboratorio local

Usa k3d para probar el operador con Fluss 1.0.0. Necesitas `k3d`, `kubectl`,
`helm`, Rust y un backend compatible con S3 accesible desde el clúster. El
operador no crea ZooKeeper, buckets ni credenciales. El
[manifiesto de RustFS](../examples/rustfs.md) usa un endpoint de ejemplo:
sustitúyelo por el tuyo antes de aplicarlo.

## Crear el clúster

```bash
k3d cluster create lab --servers 1 --agents 3
kubectl create namespace fluss
kubectl create namespace data
helm repo add bitnami https://charts.bitnami.com/bitnami
helm install zk bitnami/zookeeper --namespace fluss --version 0.15.0
kubectl apply -f deploy/crd.yaml
```

Espera a que `zk-zookeeper-0` esté listo en `fluss`. El ejemplo se conecta a
`zk-zookeeper.fluss.svc.cluster.local:2181`. Si tu instalación de ZooKeeper
usa otro Service, cambia `zookeeper.addresses` en el manifiesto.

Crea el bucket `fluss-lab` en tu backend S3. Usa credenciales de un usuario
IAM autorizado a acceder al bucket y a obtener sesiones RustFS `AssumeRole`;
las claves root y de service accounts no sirven para este flujo. Mantén las
credenciales fuera de Git. Por ejemplo, si ya tienes los archivos en tu
máquina:

```bash
kubectl -n data create secret generic fluss-rustfs \
  --from-file=access-key=/ruta/a/access-key \
  --from-file=secret-key=/ruta/a/secret-key
```

Edita una **copia local** de `docs/en/examples/rustfs.yaml`: sustituye el
endpoint S3 de ejemplo por una dirección accesible desde los pods y ajusta
el bucket y el prefijo si hace falta. Cada clúster de prueba necesita su
propio prefijo. Arranca el operador y, desde otra terminal, aplica la copia:

```bash
RUST_LOG=info cargo run --bin nightowl -- --namespace data
```

```bash
kubectl apply -f /ruta/a/rustfs-local.yaml
kubectl -n data get flussclusters,pods,pvc
kubectl -n data describe flusscluster rustfs-lab
```

Si cambia la API, reaplica `deploy/crd.yaml` antes de arrancar la nueva versión
del operador. Una CRD antigua puede rechazar condiciones nuevas o descartar
campos de `status`. `RemoteStorageReady=True` confirma que existen el Secret y
sus claves; **no** comprueba la conexión a S3 ni a STS. Consulta los logs de
los pods y escribe y lee datos con un cliente Fluss para probar ese recorrido.

## Acceso desde el host

Puede que el host no resuelva los nombres DNS de los pods ni tenga ruta hacia
sus IP. Es más sencillo ejecutar los clientes dentro de Kubernetes. Para
probar un cliente nativo desde el host, usa el
[listener externo TLS/SNI](native-external-access.md) o configura
temporalmente DNS y acceso a la red de pods de tu instalación k3d; retira
esos cambios al terminar.

## Limpiar el entorno

Borrar un `FlussCluster` no elimina sus PVC ni los objetos S3. Para comenzar
una prueba desde cero, borra el CR, elimina expresamente los PVC de **ese
clúster de prueba** si ya no necesitas los datos y borra el Secret temporal.
Si quieres descartar los datos remotos, borra también su prefijo S3 por
separado. `k3d cluster delete lab` elimina todo el clúster local, incluidos
los metadatos de ZooKeeper y los demás workloads.

## Versiones de esta configuración

| Componente | Versión |
| --- | --- |
| Servidor Fluss y manifiesto de ejemplo | 1.0.0 |
| Chart Helm de ZooKeeper | Bitnami `0.15.0` |
| Entorno k3d / k3s probado | k3d `v5.9.0` / k3s `v1.35.5+k3s1` |

El ejemplo público no se ha probado en AWS EKS. La configuración de RustFS
se ha probado con una instancia externa de RustFS 1.0.0, que estos comandos
no instalan. Para conocer los límites de autenticación y recuperación,
consulta [almacenamiento remoto](remote-storage.md).
