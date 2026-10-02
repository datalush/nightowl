<div class="cover">
  <span class="eyebrow">DATALUSH / DOCUMENTACIÓN DEL OPERADOR</span>
  <h1>Fluss<br>en Kubernetes.</h1>
  <p>Configura clústeres Apache Fluss con Night Owl. Empieza con un ejemplo, consulta la API y comprueba qué integraciones se han probado.</p>
  <span class="cover-meta">FlussCluster · fluss.datalush.com/v1alpha1 · Fluss 1.0.0 como referencia · AGPL-3.0-only</span>
</div>

## Por dónde empezar

<div class="doc-grid">
  <a class="doc-card" href="api/fluss-cluster.html"><span class="card-index">01 / EL CONTRATO</span><strong>API FlussCluster</strong><span>Campos, tipos, restricciones y propiedad de los recursos.</span><span class="card-arrow">Explorar →</span></a>
  <a class="doc-card" href="api/remote-storage.html"><span class="card-index">02 / ALMACENAMIENTO REMOTO</span><strong>S3 y credenciales</strong><span>Identidad del pod, Secrets y tokens delegados.</span><span class="card-arrow">Explorar →</span></a>
  <a class="doc-card" href="current-state.html"><span class="card-index">03 / ESTADO</span><strong>Estado actual</strong><span>Funciones implementadas e integraciones probadas.</span><span class="card-arrow">Explorar →</span></a>
</div>

## Elige un entorno

| Entorno | Origen de las credenciales | Ejemplo | Verificación |
| --- | --- | --- | --- |
| Laboratorio RustFS | Secret de Kubernetes con clave de acceso y clave secreta | [Manifiesto RustFS](examples/rustfs.md) | Verificado en vivo (convergencia, snapshots KV, SigV4/AssumeRole) |
| AWS EKS | ServiceAccount existente con IRSA o EKS Pod Identity | [Manifiesto EKS](examples/aws-eks.md) | Planificada |

Los ejemplos usan la API de `src/api.rs`. Consulta el [estado actual](current-state.md) para saber qué integraciones se han probado antes de elegir un backend.

## Cómo funciona

- **Un recurso por clúster.** `FlussCluster` y sus recursos gestionados pertenecen al mismo namespace.
- **Almacenamiento remoto explícito.** Los PVC de los TabletServers y el almacenamiento S3 compartido resuelven problemas distintos.
- **Secretos por referencia.** Los valores de las credenciales no pertenecen al recurso personalizado ni a un ConfigMap.
- **Operaciones inseguras bloqueadas.** Las actualizaciones de versión y la reducción de TabletServers requieren comprobaciones específicas de Fluss.

Para probar el operador en local, sigue la [guía del laboratorio](api/lab.md).
