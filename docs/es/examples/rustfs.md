# RustFS

**Verificado en vivo · Secret de Kubernetes · endpoint S3 propio · laboratorio**

Este manifiesto expresa el almacenamiento remoto de Fluss con el backend RustFS del laboratorio. Es el único ejemplo S3 verificado de extremo a extremo: el operador lo converge con `RemoteStorageReady=True`, los tablets escriben snapshots KV bajo el prefijo, y los flujos SigV4 y credenciales `AssumeRole` están probados contra RustFS 1.0.0. Bucket, prefijos y credenciales son los del laboratorio; aporta tu propio backend antes de reutilizar la forma.

```yaml
{{#include ../../en/examples/rustfs.yaml}}
```

Crea el namespace `data` y un Secret existente llamado `fluss-rustfs` con claves `access-key` y `secret-key`. La API contiene **referencias**, no sus valores: el operador monta el Secret como solo lectura en los pods y genera marcadores `${directory:...}` en `server.yaml`.

La restauración entre servidores y la emisión de tokens de cliente se siguen por separado; escribir en S3 con éxito no demuestra ninguno de esos flujos. Consulta [Almacenamiento remoto y credenciales](../api/remote-storage.md).
