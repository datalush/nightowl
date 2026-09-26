# AWS EKS

**Identidad del pod · Amazon S3 · Alta disponibilidad del Coordinator**

Este manifiesto muestra la API actual de `FlussCluster` con dos Coordinators, tres TabletServers y Amazon S3. El watcher actual **no lo despliega**; consulta el [estado actual](../current-state.md).

```yaml
{{#include ../../en/examples/aws-eks.yaml}}
```

## Identidad y permisos

El ServiceAccount `fluss-production` debe existir en el namespace `data`. Un administrador de AWS debe configurar **IRSA** o una **asociación EKS Pod Identity** para ese ServiceAccount; indicar su nombre en el CR no crea recursos IAM. La identidad del servidor necesita permisos sobre el bucket y su prefijo, además de `sts:AssumeRole` sobre `fluss-clients-read`.

El `roleArn` de `delegation` corresponde al **rol delegado para clientes**, no al rol del ServiceAccount. Su política de confianza debe permitir al rol del servidor asumirlo; los clientes necesitan los permisos S3 apropiados. Limita los permisos al prefijo exclusivo cuando sea posible y, si el bucket usa SSE-KMS, incluye los permisos KMS necesarios. El bucket, los roles, ZooKeeper y la StorageClass deben existir antes del despliegue.

Fluss 1.0 usa la cadena de credenciales AWS predeterminada con IRSA. EKS Pod Identity también entrega credenciales mediante esa cadena, pero todavía hay que probar la emisión de tokens de Fluss con ese mecanismo. [Referencia S3 de Fluss](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/) · [Identidad de workloads en EKS](https://docs.aws.amazon.com/eks/latest/userguide/service-accounts.html)

## Significado operativo

`spreadAcrossNodes` expresa la intención de repartir réplicas entre nodos; el programa actual no genera aún reglas de ubicación. `jvm.heap` debe dejar memoria suficiente por debajo del límite del contenedor para usos fuera del heap. Los listeners deberán determinar tanto los Services como la configuración de Fluss. El PDB solicita `maxUnavailable: 0` para tablets; `scaleIn: Block` exige además una comprobación de Fluss, y los tiempos de `rollingUpgrade` **no** activan actualizaciones por sí solos.

El factor de replicación predeterminado se aplica a **tablas nuevas**. Tener dos Coordinators no garantiza por sí solo la disponibilidad de ZooKeeper ni del bucket S3. El almacenamiento, las actualizaciones y la reducción segura requieren el ciclo de vida explicado en la [referencia de la API](../api/fluss-cluster.md#cambios-en-un-cluster-existente).
