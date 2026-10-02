# AWS EKS

**Identidad del pod · Amazon S3 · Alta disponibilidad del Coordinator · verificación planificada**

Este manifiesto muestra la API actual de `FlussCluster` con dos Coordinators, tres TabletServers y Amazon S3. El operador converge esta forma (services, config, StatefulSets, PDBs, scheduling, JVM), pero aún no se ha verificado contra AWS real; consulta el [estado actual](../current-state.md).

```yaml
{{#include ../../en/examples/aws-eks.yaml}}
```

## Identidad y permisos

El ServiceAccount `fluss-production` debe existir en el namespace `data`. Un administrador de AWS debe configurar **IRSA** o una **asociación EKS Pod Identity** para ese ServiceAccount; indicar su nombre en el CR no crea recursos IAM. La identidad del servidor necesita permisos sobre el bucket y su prefijo, además de `sts:AssumeRole` sobre `fluss-clients-read`.

El `roleArn` de `delegation` corresponde al **rol delegado para clientes**, no al rol del ServiceAccount. Su política de confianza debe permitir al rol del servidor asumirlo; los clientes necesitan los permisos S3 apropiados. Limita los permisos al prefijo exclusivo cuando sea posible y, si el bucket usa SSE-KMS, incluye los permisos KMS necesarios. El bucket, los roles, ZooKeeper y la StorageClass deben existir antes del despliegue.

Fluss 1.0 usa la cadena de credenciales AWS predeterminada con IRSA. EKS Pod Identity también entrega credenciales mediante esa cadena, pero todavía hay que probar la emisión de tokens de Fluss con ese mecanismo. [Referencia S3 de Fluss](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/) · [Identidad de workloads en EKS](https://docs.aws.amazon.com/eks/latest/userguide/service-accounts.html)

## Significado operativo

`spreadAcrossNodes` añade una regla preferente de distribución por nodo;
selectores, afinidad y tolerancias pasan a los pods. Deja margen para memoria
fuera del heap por debajo de la reserva o el límite: el operador bloquea
heaps mayores.

El ejemplo solicita `maxUnavailable: 0` para tablets. El scale-in también
exige que Fluss confirme que los servidores salientes están vacíos; Fluss
1.0 estándar no ofrece esa lectura. Las duraciones de `rollingUpgrade` no
provocan por sí solas un cambio de versión.

El factor de replicación predeterminado se aplica a **tablas nuevas**. Tener dos Coordinators no garantiza por sí solo la disponibilidad de ZooKeeper ni del bucket S3. El almacenamiento, las actualizaciones y la reducción segura requieren el ciclo de vida explicado en la [referencia de la API](../api/fluss-cluster.md#cambios-en-un-cluster-existente).
