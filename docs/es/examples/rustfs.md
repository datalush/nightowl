# RustFS

**Verificado en vivo · Secret de Kubernetes · endpoint S3 propio · laboratorio**

Este manifiesto configura el almacenamiento remoto con RustFS. El operador se ha probado con RustFS 1.0.0: los tablets escribieron snapshots KV y `AssumeRole` emitió credenciales de cliente válidas. `RemoteStorageReady=True` solo comprueba referencias; no verifica esas operaciones. El endpoint del ejemplo es ficticio: configura un backend y un bucket accesibles y credenciales de usuario IAM antes de aplicar una copia.

```yaml
{{#include ../../en/examples/rustfs.yaml}}
```

Crea el namespace `data` y un Secret existente llamado `fluss-rustfs` con claves `access-key` y `secret-key`. La API contiene **referencias**, no sus valores: el operador monta el Secret como solo lectura en los pods y genera marcadores `${directory:...}` en `server.yaml`.

Fluss recupera datos desde snapshots y réplicas supervivientes; el operador informa de la salud observada. Consulta [almacenamiento remoto y credenciales](../api/remote-storage.md) para conocer el alcance de las pruebas y sus límites.
