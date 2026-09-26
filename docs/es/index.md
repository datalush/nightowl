<div class="cover">
  <span class="eyebrow">DATALUSH / DOCUMENTACIÓN DEL OPERADOR</span>
  <h1>Opera Fluss<br>con intención.</h1>
  <p>Una API declarativa de Kubernetes para clústeres Apache Fluss. Explora el contrato del recurso, consulta manifiestos completos y conoce el estado del desarrollo sin confundir el diseño de la API con un controlador terminado.</p>
  <span class="cover-meta">FlussCluster · fluss.datalush.com/v1alpha1 · Fluss 1.0.0 como referencia</span>
</div>

## Por dónde empezar

<div class="doc-grid">
  <a class="doc-card" href="api/fluss-cluster.html"><span class="card-index">01 / EL CONTRATO</span><strong>API FlussCluster</strong><span>Campos, tipos, restricciones y propiedad de los recursos.</span><span class="card-arrow">Explorar →</span></a>
  <a class="doc-card" href="api/remote-storage.html"><span class="card-index">02 / ALMACENAMIENTO REMOTO</span><strong>S3 y credenciales</strong><span>Identidad del pod, Secrets y tokens delegados.</span><span class="card-arrow">Explorar →</span></a>
  <a class="doc-card" href="current-state.html"><span class="card-index">03 / IMPLEMENTACIÓN</span><strong>Estado actual</strong><span>Qué cubre el esquema y qué hace realmente el controlador.</span><span class="card-arrow">Explorar →</span></a>
</div>

## Elige un entorno

| Entorno | Origen de las credenciales | Ejemplo | Verificación |
| --- | --- | --- | --- |
| Laboratorio RustFS | Secret de Kubernetes con clave de acceso y clave secreta | [Manifiesto RustFS](examples/rustfs.md) | Verificado en vivo (convergencia, snapshots KV, SigV4/AssumeRole) |
| AWS EKS | ServiceAccount existente con IRSA o EKS Pod Identity | [Manifiesto EKS](examples/aws-eks.md) | Planificada |

> **Alcance de esta documentación.** Los manifiestos ilustran la API Rust actual de `operator/src/api.rs` y el operador los convierte en clústeres Fluss en ejecución. Cada ejemplo indica su estado de verificación; los backends no probados no se documentan como funcionales. Consulta el [estado actual](current-state.md) antes de aplicar un ejemplo.

## Principios de diseño

- **Kubernetes declara la intención.** `FlussCluster` pertenece a un namespace; sus recursos gestionados vivirán en él.
- **Almacenamiento remoto explícito.** Los PVC de los TabletServers y el almacenamiento S3 compartido resuelven problemas distintos.
- **Secretos por referencia.** Los valores de las credenciales no pertenecen al recurso personalizado ni a un ConfigMap.
- **Operaciones inseguras bloqueadas.** Las actualizaciones de versión y la reducción de TabletServers requieren comprobaciones específicas de Fluss.

El archivo `ROADMAP.md` del repositorio explica la estrategia general; este libro documenta la API concreta del Operador y el estado de su implementación.
