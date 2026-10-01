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

Si se omite `provider`, se aplican los defaults AWS; `provider: aws` es
equivalente. **Siempre se usa direccionamiento por ruta**; no hay un campo
para elegir el estilo S3. Un endpoint S3 personalizado **no** selecciona otro
proveedor: la delegación y STS se configuran explícitamente. `AssumeRole` es
el modo predeterminado: AWS y endpoints personalizados exigen un
`delegation.roleArn` real. `GetSessionToken` solo se usa al seleccionar
`delegation.type: getSessionToken`. Cambiar perfil o delegación exige reinicio;
cambiar bucket/prefijo requiere migrar datos. Antes de actualizar el operador
hay que adaptar o recrear los CR experimentales sin rol.

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

El rol IAM del ServiceAccount y `delegation.roleArn` **son cosas distintas**. El primero identifica al servidor Fluss y necesita acceso a S3 y permiso `sts:AssumeRole`. El segundo es el rol que Fluss asume para emitir credenciales temporales a sus clientes. Un ARN implica `AssumeRole` sin escribir `type`; `provider: aws` es opcional. Fluss 1.0 exige un rol asumible cuando el servidor obtiene sus credenciales de la cadena predeterminada de AWS. EKS Pod Identity usa el proveedor de credenciales para contenedores del SDK; hay que probar su integración con Fluss en EKS antes de declararla verificada.

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

Fluss resuelve los marcadores al arrancar, por lo que rotar el Secret exige reiniciar los servidores afectados. El montaje está implementado (verificado en vivo contra RustFS). La rotación se detecta, no se cura: los pods fijan el hash del Secret en su plantilla, y si difiere del Secret vivo aparece `S3CredentialsStale=True` nombrando los pods afectados y el montaje stale — sin reiniciar nada. La política de reinicio va por separado. Las claves estáticas AWS también requieren rol salvo selección explícita de `getSessionToken`.

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

Que el servidor lea y escriba objetos S3 no demuestra que Fluss pueda emitir credenciales para clientes Flink/Spark que leen datos remotos. El fallback propio de Fluss 1.0 llama a `GetSessionToken` con claves estáticas; el Operador **no** lo utiliza en silencio: predetermina `AssumeRole` y exige un `delegation.roleArn` real para AWS/servicios personalizados. `type: getSessionToken` lo selecciona explícitamente cuando el backend lo soporte. Con identidad de pod hace falta `delegation.roleArn` explícito. En servicios S3 compatibles, `stsEndpoint` sobrescribe el endpoint STS en ambos modos. El perfil no crea identidades IAM, roles, buckets ni un endpoint accesible a workers Spark externos: son entradas de plataforma.

| Backend | Qué expresa la API | Qué queda por verificar |
| --- | --- | --- |
| AWS S3 | Identidad EKS con `assumeRole`; claves estáticas con el modo STS elegido. | Planificado: confianza y permisos IAM, tokens para clientes, snapshots, recuperación y failover. |
| RustFS | Secret, endpoint propio, acceso por ruta, AssumeRole. | Verificado el 2026-09-26 contra la instancia del lab (1.0.0): round-trip de bucket desde pods del clúster, credenciales `AssumeRole` que autorizan operaciones S3, snapshots KV escritos por tablets gestionadas bajo el prefijo del clúster y recuperación de un disco perdido desde esos snapshots (ver abajo). Los tokens emitidos por Fluss quedan aparte. |

Si se omite `delegation` con AWS u otro endpoint, el operador bloquea los nuevos workloads con un motivo claro en vez de llamar a AWS STS silenciosamente. El CRD exige rol con identidad de pod y rechaza `getSessionToken` combinado con rol. Hay que volver a aplicar el CRD generado antes de usar perfiles u omitir `delegation.type`: uno anterior puede rechazar la forma nueva. El reconciler verifica el Secret o ServiceAccount y bloquea combinaciones inválidas antes de iniciar workloads; `RemoteStorageReady` solo observa **referencias**, no conectividad S3/STS ni red externa. Las operaciones S3, los snapshots KV, la recuperación de un disco y el flujo de tokens emitidos por Fluss se probaron contra RustFS con usuario IAM (pruebas separadas); AWS sigue sin probarse. Los ejemplos no son certificaciones de compatibilidad.

## Recuperación desde snapshots remotos

Perder el disco de un TabletServer con réplicas supervivientes se recupera de forma nativa: el pod de reemplazo se reincorpora y se pone al día desde sus pares más los snapshots KV remotos, sin orquestación del operador. Verificado el 2026-09-26 contra el RustFS del lab (RF=3, 100 filas escritas, un PVC más su pod borrados): el pod nuevo estuvo Ready en unos cuatro minutos sin pasos manuales, las condiciones se mantuvieron veraces durante el proceso y las 100 filas se leyeron después. El papel del operador es observar e informar mediante `ClusterHealthy` y las condiciones por área. La pérdida total sin réplica viva en ningún sitio no la recupera ningún flujo —no hay de dónde recuperar— y queda como límite documentado, no como trabajo pendiente.

## Flujo de tokens de cliente vía AssumeRole

Con `delegation: { type: assumeRole, roleArn, stsEndpoint }` los servidores obtienen credenciales S3 de sesión vía AssumeRole contra el STS del backend (el log muestra `S3DelegationTokenProvider … Obtaining session credentials via AssumeRole`), y los clientes piden tokens de seguridad del filesystem por el RPC `GetFileSystemSecurityToken`. Verificado el 2026-09-27 contra el RustFS del lab con un usuario IAM acotado al lab (`fluss-clients`, política limitada al bucket del lab): 50 filas escritas y leídas, snapshots fluyendo, y un token pedido por el cliente (access key, secreto, JWT de sesión) listando y leyendo un objeto `_METADATA` real de snapshot. El backend acepta el ARN del rol por compatibilidad AWS y deriva la sesión de las políticas de la credencial firmante; no hizo falta sentencia explícita `sts:AssumeRole` (el validador de políticas rechaza `Resource: "*"`).

Fuentes: [configuración S3 de Fluss 1.0](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/), [proveedores de secretos de Fluss 1.0](https://fluss.apache.org/docs/security/secrets/), [IRSA de EKS](https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html), [EKS Pod Identity](https://docs.aws.amazon.com/eks/latest/userguide/pod-identities.html) y [documentación STS de RustFS](https://docs.rustfs.com/en/security-compliance/iam/sts).
