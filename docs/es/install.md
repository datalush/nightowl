# Instalación del operador

Sin toolchain Rust. Todo se aplica con `kubectl` directamente desde los
manifiestos versionados en `deploy/`:

```bash
kubectl apply -f deploy/crd.yaml
kubectl create namespace operator-system
kubectl apply -f deploy/serviceaccount.yaml
kubectl apply -f deploy/clusterrole.yaml
kubectl apply -f deploy/clusterrolebinding.yaml
kubectl apply -f deploy/deployment.yaml
```

La CRD se genera desde `src/api.rs` (`cargo run --bin gen-crd`). Al actualizar
el operador, aplica los manifiestos nuevos. Antes de aplicar el Deployment,
fija su `image:` a una imagen publicada. Por defecto el operador vigila todos
los namespaces; pasa `--namespace <nombre>` para limitarlo a uno.

Para probar un clúster, crea el namespace `data`, su Secret S3 y el bucket.
Copia el manifiesto RustFS y sustituye el endpoint de ejemplo por una
dirección accesible desde los pods. Aplica después tu copia local:

```bash
kubectl apply -f /ruta/a/rustfs-local.yaml
kubectl get flussclusters -A
```

Consulta [FlussCluster](api/fluss-cluster.md) para el spec completo y
[Estado y condiciones](api/status.md) para saber qué observar mientras converge.
