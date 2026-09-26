# Estado actual

El esquema de `FlussCluster` se define en `operator/src/api.rs`. Es un **contrato de despliegue propuesto**, no la afirmación de que todas sus funciones ya están operativas. Este libro sigue los tipos Rust y distingue las capacidades del servidor Fluss de las del Operador.

| Capacidad | Estado actual |
| --- | --- |
| Definir y serializar `FlussCluster` | Implementado en Rust; el esquema ha cambiado desde que se instaló la primera CRD en el entorno de desarrollo. |
| Observar recursos personalizados | `src/main.rs` observa `FlussCluster` en `operator-dev` e imprime nombres y versiones. |
| Observar todos los namespaces | No implementado; el proceso actual se limita a `operator-dev`. |
| Crear CoordinatorServers o TabletServers | No implementado. El laboratorio Fluss existente lo gestiona Helm, no este Operador. |
| Configurar S3 o montar Secrets | Representado en la API; todavía no hay un despliegue que use estos campos. |
| Actualizar `.status` | Existe el tipo Rust; ningún controlador escribe aún el subrecurso de estado. |
| Reinicio, actualización, reducción o recuperación seguros | No implementados. Cambiar `spec` no implica que estas operaciones se ejecuten. |

## Cómo utilizar los ejemplos

Los manifiestos son **ejemplos de la API**, no instrucciones de instalación. Sus campos siguen el modelo Rust, pero la CRD instalada puede conservar un esquema anterior. Aplicar un recurso no despliega Fluss mientras no exista reconciliación. La admisión de Kubernetes tampoco demuestra compatibilidad de almacenamiento o de IAM.

Las páginas de [EKS](examples/aws-eks.md), [MinIO](examples/minio.md) y [Garage](examples/garage.md) explican las distintas opciones de autenticación y delegación. Tener una URI S3 no demuestra que Fluss pueda emitir tokens delegados ni recuperar una tableta KV en otro servidor.

## Documentación local

Desde `operator/`, construye ambos idiomas y sirve la salida estática:

```sh
./docs/build-docs.sh serve
```

Abre `http://localhost:3000/en/` o `http://localhost:3000/es/` (puedes cambiar el puerto con `DOCS_PORT`). Ambos idiomas se sirven desde el mismo servidor. `mdbook serve` solo sirve **un idioma**: su URL `/es/` devolverá 404. Los HTML se generan en `target/book/en/` y `target/book/es/`. Las páginas están en `docs/en/` y `docs/es/`; los manifiestos YAML se mantienen en `docs/en/examples/*.yaml` y ambas traducciones los incluyen.
