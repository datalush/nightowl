# Almacenamiento remoto y credenciales

Fluss usa discos locales de los TabletServers para los datos recientes y **almacenamiento remoto compartido** para snapshots KV y segmentos de log remotos. Son capas distintas. En modo distribuido, una ruta local en cada pod no sustituye una ubicación S3 compartida. La API actual acepta una ubicación compatible con S3 en `spec.remoteStorage.s3`.

## Ubicación S3

| Campo | Tipo | Obligatorio | Significado |
| --- | --- | --- | --- |
| `provider` | `aws` o `rustfs` | No | Perfil de defaults. Omitido equivale a `aws`; los campos explícitos prevalecen. |
| `bucket` | cadena | Sí | Bucket existente; el Operador no lo crea. |
| `prefix` | cadena | Sí | Prefijo exclusivo para este `FlussCluster`. No debe compartirse con otro clúster. |
| `region` | cadena | Sí | Región que recibe el plugin S3 de Fluss. |
| `endpoint` | URL | No | Endpoint S3 compatible, p. ej. el RustFS del laboratorio. |
| `authentication` | objeto con discriminador | Sí | `workloadIdentity` o `secret`. |
| `delegation` | objeto | No | `roleArn` implica `AssumeRole`; `type` opcional elige el modo y `stsEndpoint` sobreescribe STS. |

Si omites `provider`, se usan los valores predeterminados de AWS. El acceso S3
siempre es por ruta. Un endpoint personalizado no activa el perfil RustFS:
configura su delegación y su endpoint STS expresamente.

AWS y los endpoints personalizados exigen un `delegation.roleArn` real para el
modo `AssumeRole` predeterminado. Usa `delegation.type: getSessionToken` solo
si el backend lo admite. Cambiar proveedor o delegación requiere un reinicio;
cambiar bucket o prefijo requiere migrar los datos.

El reconciler convierte `bucket` y `prefix` en `s3://<bucket>/<prefix>` dentro de la clave singular `remote.data.dir` y configura `s3.region`, `s3.endpoint` y `s3.path-style-access` cuando corresponde. La clave singular es deliberada: la imagen `apache/fluss:1.0.0` ignora el plural `remote.data.dirs` y aborta el arranque con ruta remota nula (ver ADR-0001). Tras escribir datos, cambiar la ubicación requiere una migración, no simplemente generar otra configuración para los pods.

## Autenticación del servidor

### Identidad del pod en EKS

```yaml
authentication:
  type: workloadIdentity
  serviceAccountName: fluss-production
delegation:
  roleArn: arn:aws:iam::123456789012:role/fluss-clients-read
```

`serviceAccountName` es un **ServiceAccount existente en el namespace de FlussCluster**. El administrador de AWS lo asocia a un rol IAM mediante IRSA o EKS Pod Identity; el Operador no crea roles IAM ni asociaciones Pod Identity. Los pods Fluss usan la cadena de credenciales predeterminada del SDK de AWS, sin `s3.access-key` ni `s3.secret-key`.

En concreto, con `workloadIdentity` no se genera **ninguna clave de credenciales**: ni `s3.access-key`, ni `s3.secret-key`, ni bloque `config.providers`. La única clave relacionada con la identidad procede de `delegation` (`s3.assumed.role.arn`, obligatoria en este modo).

El rol IAM del ServiceAccount identifica al servidor Fluss: necesita acceso a
S3 y permiso para asumir el rol de cliente. `delegation.roleArn` identifica
**ese rol de cliente**, con el que se emiten credenciales temporales. Son
roles distintos. Indicar un ARN selecciona `AssumeRole` sin añadir `type`.

Fluss 1.0 exige ese rol cuando el servidor obtiene las credenciales de la
cadena predeterminada de AWS. EKS Pod Identity usa el proveedor de
credenciales para contenedores del SDK; su integración con Fluss aún no se
ha probado en EKS.

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

Fluss resuelve los marcadores al arrancar, así que al rotar el Secret hay
que reiniciar los servidores. El operador compara el Secret con el hash
fijado en cada pod y muestra `S3CredentialsStale=True` si no coinciden; no
reinicia los pods por la rotación. Las claves estáticas AWS también requieren
un rol, salvo que se seleccione `getSessionToken` expresamente.

### Perfil RustFS

```yaml
provider: rustfs
bucket: fluss
prefix: clusters/dev
region: us-east-1
endpoint: https://storage.example.com
authentication:
  type: secret
  secretRef: {name: fluss-rustfs, accessKeyKey: access-key, secretKeyKey: secret-key}
```

El operador siempre emite acceso por ruta; el perfil RustFS añade `AssumeRole` y STS en el mismo `endpoint`.
RustFS 1.0 acepta el RoleArn convencional del perfil por compatibilidad AWS;
ese nombre **no concede** permisos. Se necesitan claves permanentes de un
**usuario IAM** RustFS con política aplicable al bucket: ni root ni las
service accounts pueden llamar a `AssumeRole`. Se pueden sobrescribir
`delegation.roleArn` y `delegation.stsEndpoint`. El operador comprueba el Secret, pero **no** prueba que STS emita
tokens: `RemoteStorageReady=True` no significa que los clientes estén listos.

## La delegación es otro requisito de compatibilidad

Que un servidor escriba en S3 no demuestra que los clientes Flink o Spark
puedan obtener credenciales temporales para leer datos remotos. Para AWS y
endpoints personalizados, el operador usa `AssumeRole` con un `roleArn` real;
no cambia a `GetSessionToken` sin pedirlo. La identidad de pod también exige
un ARN. `stsEndpoint` permite cambiar la dirección de STS. Las identidades
IAM, los buckets y el acceso de red para workers externos se configuran aparte.

| Backend | Qué expresa la API | Qué queda por verificar |
| --- | --- | --- |
| AWS S3 | Identidad EKS con `assumeRole`; claves estáticas con el modo STS elegido. | Planificado: confianza y permisos IAM, tokens para clientes, snapshots, recuperación y failover. |
| RustFS | Secret, endpoint propio, acceso por ruta, AssumeRole. | Probado con RustFS 1.0.0: escrituras S3, snapshots KV, reemplazo de disco y tokens de cliente. Consulta los límites más abajo. |

Si se omite la delegación con AWS u otro endpoint personalizado, el operador
bloquea los nuevos pods. La CRD exige un rol con identidad de pod y rechaza
`getSessionToken` combinado con un rol. Al actualizar, instala la CRD
correspondiente. El operador comprueba las referencias al Secret o al
ServiceAccount antes de arrancar; `RemoteStorageReady` **no** comprueba S3,
STS ni el acceso desde fuera. Los flujos RustFS se probaron con claves de
usuario IAM; AWS sigue sin probarse.

## Recuperación desde snapshots remotos

Fluss puede recuperar el disco perdido de un TabletServer a partir de otras
réplicas y snapshots KV remotos, sin que el operador mueva datos. Se ha
probado con RustFS, incluido el reemplazo y promoción de un follower. El
operador informa del estado. Con RF=1, recuperar un snapshot **no** demuestra
que se hayan conservado todas las escrituras confirmadas: comprueba
`DataAtRisk`. Los snapshots no garantizan pérdida cero si desaparecen todas
las réplicas vivas.

## Flujo de tokens de cliente vía AssumeRole

Con `AssumeRole`, Fluss obtiene credenciales de sesión del STS del backend.
Los clientes piden tokens mediante `GetFileSystemSecurityToken`. En RustFS,
un token emitido a partir de un usuario IAM con acceso al bucket permitió
leer un objeto de un snapshot KV. Esto no verifica IAM en AWS ni la red de
workers externos. RustFS acepta el ARN convencional del perfil por
compatibilidad; los permisos proceden de la política del usuario IAM.

Fuentes: [configuración S3 de Fluss 1.0](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/), [proveedores de secretos de Fluss 1.0](https://fluss.apache.org/docs/security/secrets/), [IRSA de EKS](https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html), [EKS Pod Identity](https://docs.aws.amazon.com/eks/latest/userguide/pod-identities.html) y [documentación STS de RustFS](https://docs.rustfs.com/en/security-compliance/iam/sts).
