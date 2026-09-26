# Almacenamiento remoto y credenciales

Fluss usa discos locales de los TabletServers para los datos recientes y **almacenamiento remoto compartido** para snapshots KV y segmentos de log remotos. Son capas distintas. En modo distribuido, una ruta local en cada pod no sustituye una ubicación S3 compartida. La API actual acepta una ubicación compatible con S3 en `spec.remoteStorage.s3`.

## Ubicación S3

| Campo | Tipo | Obligatorio | Significado |
| --- | --- | --- | --- |
| `bucket` | cadena | Sí | Bucket existente; el Operador no lo crea. |
| `prefix` | cadena | Sí | Prefijo exclusivo para este `FlussCluster`. No debe compartirse con otro clúster. |
| `region` | cadena | Sí | Región que recibe el plugin S3 de Fluss. |
| `endpoint` | URL | No | Endpoint S3 compatible, normalmente MinIO o Garage. |
| `pathStyleAccess` | booleano | No | `false` por defecto; utiliza `true` con endpoints S3 compatibles locales. |
| `authentication` | objeto con discriminador | Sí | `workloadIdentity` o `secret`. |
| `delegation` | objeto con discriminador | No | `assumeRole` o `getSessionToken`; consulta la distinción siguiente. |

El futuro reconciler convertirá `bucket` y `prefix` en `s3://<bucket>/<prefix>` y configurará `s3.region`, `s3.endpoint` y `s3.path-style-access` cuando corresponda. Fluss 1.0 recomienda `remote.data.dirs` para clústeres nuevos, incluso con una sola ubicación. Tras escribir datos, cambiar la ubicación requiere una migración, no simplemente generar otra configuración para los pods.

## Autenticación del servidor

### Identidad del pod en EKS

```yaml
authentication:
  type: workloadIdentity
  serviceAccountName: fluss-production
delegation:
  type: assumeRole
  roleArn: arn:aws:iam::123456789012:role/fluss-clients-read
```

`serviceAccountName` es un **ServiceAccount existente en el namespace de FlussCluster**. El administrador de AWS lo asocia a un rol IAM mediante IRSA o EKS Pod Identity; el Operador no crea roles IAM ni asociaciones Pod Identity. Los pods Fluss usan la cadena de credenciales predeterminada del SDK de AWS, sin `s3.access-key` ni `s3.secret-key`.

En concreto, con `workloadIdentity` no se genera **ninguna clave de credenciales**: ni `s3.access-key`, ni `s3.secret-key`, ni bloque `config.providers`. La única clave relacionada con la identidad procede de `delegation` (`s3.assumed.role.arn`, obligatoria en este modo).

El rol IAM del ServiceAccount y `delegation.roleArn` **son cosas distintas**. El primero identifica al servidor Fluss y necesita acceso a S3 y permiso `sts:AssumeRole`. El segundo es el rol que Fluss asume para emitir credenciales temporales a sus clientes. Fluss 1.0 exige un rol asumible cuando el servidor obtiene sus credenciales de la cadena predeterminada de AWS. EKS Pod Identity usa el proveedor de credenciales para contenedores del SDK; hay que probar su integración con Fluss en EKS antes de declararla verificada.

### Secret de Kubernetes existente

```yaml
authentication:
  type: secret
  secretRef:
    name: fluss-s3
    accessKeyKey: access-key
    secretKeyKey: secret-key
delegation:
  type: getSessionToken
```

El Secret debe existir en el **mismo namespace** que FlussCluster. Los futuros pods lo montarán como archivos de solo lectura y el ConfigMap contendrá marcadores de Fluss, no los valores de las credenciales:

```yaml
config.providers: directory
config.providers.directory.param.allowed.paths: /etc/fluss/secrets
s3.access-key: ${directory:/etc/fluss/secrets/s3:access-key}
s3.secret-key: ${directory:/etc/fluss/secrets/s3:secret-key}
```

Fluss resuelve los marcadores al arrancar, por lo que rotar el Secret exige reiniciar los servidores afectados. **El montaje y el reinicio aún no están implementados.**

## La delegación es otro requisito de compatibilidad

Que el servidor lea y escriba objetos S3 no demuestra que Fluss pueda emitir credenciales para clientes Flink/Spark que leen datos remotos. El proveedor de tokens S3 de Fluss 1.0 llama a **`GetSessionToken`** con claves estáticas por defecto o a **`AssumeRole`** cuando se le indica un ARN. Con identidad de pod es obligatorio `AssumeRole`. Para servicios S3 compatibles, `assumeRole` admite también `stsEndpoint` para el endpoint STS del servicio.

| Backend | Qué expresa la API | Qué queda por verificar |
| --- | --- | --- |
| AWS S3 | Identidad EKS con `assumeRole`; claves estáticas con el modo STS elegido. | Confianza y permisos IAM, tokens para clientes, snapshots, recuperación y failover. |
| MinIO | Secret, endpoint propio, acceso por ruta y modo STS elegido. | Que la **versión y configuración concreta de MinIO** admita la petición STS de Fluss y las credenciales devueltas. |
| Garage | Secret, endpoint propio y acceso por ruta. | Sus operaciones S3 documentadas no acreditan compatibilidad con la delegación STS de Fluss. No implica compatibilidad KV o de clientes. |

`delegation` es opcional con claves estáticas; omitirlo **no** demuestra que el backend emita tokens. La CRD generada incluye reglas CEL que exigen `assumeRole` con `workloadIdentity` y comprueban que `secretRef`, `serviceAccountName` y `roleArn` concuerden con el `type` elegido. La CRD instalada previamente no tendrá esas reglas hasta actualizarse. El futuro reconciler deberá comprobar además el Secret, el ServiceAccount y el almacenamiento remoto reales antes de declarar saludable el clúster. Los ejemplos son manifiestos de la API, no certificaciones de compatibilidad.

Fuentes: [configuración S3 de Fluss 1.0](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/), [proveedores de secretos de Fluss 1.0](https://fluss.apache.org/docs/security/secrets/), [IRSA de EKS](https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html), [EKS Pod Identity](https://docs.aws.amazon.com/eks/latest/userguide/pod-identities.html) y [matriz S3 de Garage](https://garagehq.deuxfleurs.fr/documentation/reference-manual/s3-compatibility/).
