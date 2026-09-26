# Estado actual

El esquema de `FlussCluster` se define en `operator/src/api.rs` y el controlador lo convierte en clústeres Fluss en ejecución. Lo siguiente está verificado en vivo en k3d salvo indicación contraria.

| Capacidad | Estado actual |
| --- | --- |
| Definir y validar `FlussCluster` | Implementado; reglas CEL aplicadas en vivo, CRD versionada con test de deriva. |
| Observar todos los namespaces (o uno con flag) | Implementado; clústeres del mismo nombre aislados por namespace, verificado. |
| Ejecutar CoordinatorServers y TabletServers | Implementado; identidad por ordinal, config montada, rollouts con hash. |
| Configurar S3 y montar Secrets | Implementado y verificado contra RustFS (los snapshots KV llegan al bucket). |
| RBAC de mínimo privilegio | Implementado; verificado con su ServiceAccount sin ningún `Forbidden`. |
| Actualizar `.status` | Implementado: recursos, almacenamiento, alcanzabilidad y salud con evidencia. |
| JVM heap, PVCs, PDBs, scheduling, Service cliente | Implementado; heaps sobredimensionados bloqueados antes de crear pods. |
| Reinicio, actualización, reducción o recuperación seguros | Parcial: reinicios y recuperación verificados; actualización controlada y scale-in pendientes. |
| Restore entre servidores, tokens de cliente, AWS real | Aún sin verificar; se sigue por separado. |

## Cómo utilizar los ejemplos

Los manifiestos despliegan clústeres reales; cada uno indica su estado de verificación. Las páginas de [EKS](examples/aws-eks.md) y [RustFS](examples/rustfs.md) explican las distintas opciones de autenticación y delegación. Tener una URI S3 no demuestra que Fluss pueda emitir tokens delegados ni recuperar una tableta KV en otro servidor.

## Documentación local

Desde `operator/`, construye ambos idiomas y sirve la salida estática:

```sh
./docs/build-docs.sh serve
```

Abre `http://localhost:3000/en/` o `http://localhost:3000/es/` (puedes cambiar el puerto con `DOCS_PORT`). Ambos idiomas se sirven desde el mismo servidor. `mdbook serve` solo sirve **un idioma**: su URL `/es/` devolverá 404. Los HTML se generan en `target/book/en/` y `target/book/es/`. Las páginas están en `docs/en/` y `docs/es/`; los manifiestos YAML se mantienen en `docs/en/examples/*.yaml` y ambas traducciones los incluyen.
