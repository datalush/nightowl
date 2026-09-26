# Instalación del operador

Sin toolchain Rust. Todo se aplica con `kubectl` directamente desde los
manifiestos versionados en `deploy/`:

```bash
kubectl apply -f deploy/crd.yaml
kubectl apply -f deploy/serviceaccount.yaml
kubectl apply -f deploy/clusterrole.yaml
kubectl apply -f deploy/clusterrolebinding.yaml
kubectl apply -f deploy/deployment.yaml
```

El manifiesto del CRD se genera desde `src/api.rs` (`cargo run --bin gen-crd`)
y un test falla si diverge, así que reinstalar tras una actualización son los
mismos cuatro comandos con los ficheros nuevos. Fija `image:` en
`deployment.yaml` a una build publicada; el operador vigila todos los
namespaces por defecto (pasa `--namespace <nombre>` para fijar uno).

Un clúster mínimo, tras crear el namespace y el Secret S3 que referencie:

```bash
kubectl apply -f docs/en/examples/minio.yaml
kubectl get flussclusters -A
```

Consulta [FlussCluster](api/fluss-cluster.md) para el spec completo y
[Estado y condiciones](api/status.md) para saber qué observar mientras converge.
