# MinIO

**Secret de Kubernetes · endpoint S3 personalizado · laboratorio local**

Este manifiesto describe almacenamiento remoto Fluss con un endpoint S3 de MinIO. El hostname, el bucket y las credenciales son ejemplos: proporciona un Service MinIO y un bucket reales antes de probarlo. El Operador no instala MinIO.

```yaml
{{#include ../../en/examples/minio.yaml}}
```

Prepara el namespace `data` y un Secret existente llamado `fluss-minio` con las claves `access-key` y `secret-key`. La API contiene **referencias**, no los valores. El futuro controlador montará el Secret como solo lectura y escribirá marcadores `${directory:...}` en la configuración de Fluss; aún no lo hace. Los listeners, el PDB, la JVM y `scaleIn: Block` del manifiesto también son intención de la API, no operaciones implementadas.

## La delegación requiere otra prueba

Este ejemplo **omite `delegation`** deliberadamente. Define la configuración S3 del servidor, no un mecanismo verificado para entregar tokens a los clientes. Fluss 1.0 llama a `GetSessionToken` con claves estáticas salvo que se configure `AssumeRole`; escribir un objeto S3 no demuestra que ninguno de los dos caminos STS funcione con la versión de MinIO elegida. Si ese despliegue acepta la petición `AssumeRole` que envía Fluss, el esquema permite expresarlo:

```yaml
delegation:
  type: assumeRole
  roleArn: <rol-aceptado-por-tu-STS>
  stsEndpoint: http://minio.storage.svc.cluster.local:9000
```

Estos campos irían **dentro de `spec.remoteStorage.s3`**, no al nivel superior. Verifica la emisión de tokens, los snapshots KV remotos, la recuperación en otro TabletServer y las lecturas remotas antes de confiar en este despliegue para cargas duraderas. Consulta [almacenamiento remoto y credenciales](../api/remote-storage.md).
