# Estado actual

La API de `FlussCluster` se define en `src/api.rs`. El operador se ha probado en k3d; la tabla indica cuándo falta probar un backend concreto.

| Capacidad | Estado actual |
| --- | --- |
| Definir y validar `FlussCluster` | Implementado; reglas CEL aplicadas en vivo, CRD versionada con test de deriva. |
| Observar todos los namespaces (o uno con flag) | Implementado; clústeres del mismo nombre aislados por namespace, verificado. |
| Ejecutar CoordinatorServers y TabletServers | Implementado; identidad por ordinal, config montada, rollouts con hash. |
| Configurar S3 y montar Secrets | Implementado y verificado contra RustFS (los snapshots KV llegan al bucket). |
| RBAC de mínimo privilegio | Implementado; verificado con su ServiceAccount sin ningún `Forbidden`. |
| Actualizar `.status` | Implementado: recursos, almacenamiento, alcanzabilidad y salud con evidencia. |
| JVM heap, PVCs, PDBs, scheduling, Service cliente | Implementado; heaps sobredimensionados bloqueados antes de crear pods. |
| Reinicios y scale-in | Reinicios secuenciados implementados; el scale-in exige una lectura reciente que demuestre que los servidores salientes están registrados y vacíos. Fluss 1.0 estándar no ofrece esa lectura. No hay rebalanceo automático. |
| Recuperación y tokens de cliente | Reemplazo de disco y promoción de un follower probados con RustFS; tokens S3 emitidos por Fluss probados con credenciales de usuario IAM de RustFS. La recuperación la realiza Fluss, no el operador. |
| AWS EKS | Hay un manifiesto de ejemplo; IAM, tokens y recuperación aún no se han probado en AWS. |

## Cómo utilizar los ejemplos

Los ejemplos de [EKS](examples/aws-eks.md) y [RustFS](examples/rustfs.md) requieren credenciales distintas. Ni una URI S3 ni `RemoteStorageReady=True` demuestran que Fluss pueda emitir tokens o recuperar datos. Sustituye el endpoint de ejemplo del manifiesto RustFS antes de usarlo.

## Documentación local

Desde `operator/`, construye ambos idiomas y sirve la salida estática:

```sh
./docs/build-docs.sh serve
```

Abre `http://localhost:3000/en/` o `http://localhost:3000/es/` (puedes cambiar el puerto con `DOCS_PORT`). Ambos idiomas se sirven desde el mismo servidor. `mdbook serve` solo sirve **un idioma**: su URL `/es/` devolverá 404. Los HTML se generan en `target/book/en/` y `target/book/es/`. Las páginas están en `docs/en/` y `docs/es/`; los manifiestos YAML se mantienen en `docs/en/examples/*.yaml` y ambas traducciones los incluyen.
