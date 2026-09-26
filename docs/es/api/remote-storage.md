# Almacenamiento remoto y credenciales

Fluss usa discos locales de los TabletServers para los datos recientes y **almacenamiento remoto compartido** para snapshots KV y segmentos de log remotos. Son capas distintas. En modo distribuido, una ruta local en cada pod no sustituye una ubicación S3 compartida. La API actual acepta una ubicación compatible con S3 en `spec.remoteStorage.s3`.

## Ubicación S3

| Campo | Tipo | Obligatorio | Significado |
| --- | --- | --- | --- |
| `bucket` | cadena | Sí | Bucket existente; el Operador no lo crea. |
| `prefix` | cadena | Sí | Prefijo exclusivo para este `FlussCluster`. No debe compartirse con otro clúster. |
| `region` | cadena | Sí | Región que recibe el plugin S3 de Fluss. |
| `endpoint` | URL | No | Endpoint S3 compatible, p. ej. el RustFS del laboratorio. |
| `pathStyleAccess` | booleano | No | `false` por defecto; utiliza `true` con endpoints S3 compatibles locales. |
| `authentication` | objeto con discriminador | Sí | `workloadIdentity` o `secret`. |
| `delegation` | objeto con discriminador | No | `assumeRole` o `getSessionToken`; consulta la distinción siguiente. |

El reconciler convierte `bucket` y `prefix` en `s3://<bucket>/<prefix>` dentro de la clave singular `remote.data.dir` y configura `s3.region`, `s3.endpoint` y `s3.path-style-access` cuando corresponde. La clave singular es deliberada: la imagen `apache/fluss:1.0.0` ignora el plural `remote.data.dirs` y aborta el arranque con ruta remota nula (ver ADR-0001). Tras escribir datos, cambiar la ubicación requiere una migración, no simplemente generar otra configuración para los pods.

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

El Secret debe existir en el **mismo namespace** que FlussCluster. Los pods lo montan como archivos de solo lectura y el ConfigMap contiene marcadores de Fluss, no los valores de las credenciales:

```yaml
config.providers: directory
config.providers.directory.param.allowed.paths: /etc/fluss/secrets
s3.access-key: ${directory:/etc/fluss/secrets/s3:access-key}
s3.secret-key: ${directory:/etc/fluss/secrets/s3:secret-key}
```

Fluss resuelve los marcadores al arrancar, por lo que rotar el Secret exige reiniciar los servidores afectados. El montaje está implementado (verificado en vivo contra RustFS). La rotación se detecta, no se cura: los pods fijan el hash del Secret en su plantilla, y si difiere del Secret vivo aparece `S3CredentialsStale=True` nombrando los pods afectados y el montaje stale — sin reiniciar nada. La política de reinicio va por separado.

## La delegación es otro requisito de compatibilidad

Que el servidor lea y escriba objetos S3 no demuestra que Fluss pueda emitir credenciales para clientes Flink/Spark que leen datos remotos. El proveedor de tokens S3 de Fluss 1.0 llama a **`GetSessionToken`** con claves estáticas por defecto o a **`AssumeRole`** cuando se le indica un ARN. Con identidad de pod es obligatorio `AssumeRole`. Para servicios S3 compatibles, `assumeRole` admite también `stsEndpoint` para el endpoint STS del servicio.

| Backend | Qué expresa la API | Qué queda por verificar |
| --- | --- | --- |
| AWS S3 | Identidad EKS con `assumeRole`; claves estáticas con el modo STS elegido. | Planificado: confianza y permisos IAM, tokens para clientes, snapshots, recuperación y failover. |
| RustFS | Secret, endpoint propio, acceso por ruta, AssumeRole. | Verificado el 2026-09-26 contra la instancia del lab (1.0.0): round-trip de bucket desde pods del clúster, credenciales `AssumeRole` que autorizan operaciones S3 y snapshots KV escritos por tablets gestionadas bajo el prefijo del clúster. La restauración entre servidores y los tokens emitidos por Fluss quedan aparte. |

`delegation` es opcional con claves estáticas; omitirlo **no** demuestra que el backend emita tokens. La CRD generada incluye reglas CEL que exigen `assumeRole` con `workloadIdentity` y comprueban que `secretRef`, `serviceAccountName` y `roleArn` concuerden con el `type` elegido. La CRD instalada previamente no tendrá esas reglas hasta actualizarse. El reconciler comprueba el Secret real (existencia más claves referenciadas) y el ServiceAccount antes de informar `RemoteStorageReady`, y si falta algo lo rechaza con evidencia que lo nombra; que un Secret o ServiceAccount aparezca después no re-dispara la reconciliación por sí solo —lo hace el siguiente evento observado. Las operaciones S3 básicas y los snapshots KV están verificados contra el RustFS del lab; la restauración entre servidores y el flujo de tokens emitido por Fluss se siguen por separado. Los ejemplos son manifiestos de la API, no certificaciones de compatibilidad.

Fuentes: [configuración S3 de Fluss 1.0](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/), [proveedores de secretos de Fluss 1.0](https://fluss.apache.org/docs/security/secrets/), [IRSA de EKS](https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html), [EKS Pod Identity](https://docs.aws.amazon.com/eks/latest/userguide/pod-identities.html) y [documentación STS de RustFS](https://docs.rustfs.com/en/security-compliance/iam/sts).
