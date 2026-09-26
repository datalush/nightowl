# Garage

**Secret de Kubernetes · endpoint S3 personalizado · compatibilidad experimental**

Garage admite claves de acceso S3 y peticiones de objetos por ruta (*path-style*). Este manifiesto expresa esos ajustes en la API actual de `FlussCluster`; **no demuestra compatibilidad completa con Fluss**.

```yaml
{{#include ../../en/examples/garage.yaml}}
```

Prepara el namespace `data`, un bucket Garage y un Secret `fluss-garage` en ese namespace con las entradas `access-key` y `secret-key`. Concede a la clave acceso al bucket y ajusta `region` a la región configurada en Garage (`garage` es el valor predeterminado documentado). El endpoint debe ser el de la API S3 accesible desde Kubernetes, no una interfaz web o administrativa. La JVM, los listeners, el PDB y `scaleIn: Block` todavía describen solo la intención.

## Incógnita: credenciales temporales

El manifiesto omite `delegation` deliberadamente. Fluss 1.0 emplea STS `GetSessionToken` o `AssumeRole` para emitir credenciales a clientes que acceden a datos remotos. La [matriz S3 de Garage](https://garagehq.deuxfleurs.fr/documentation/reference-manual/s3-compatibility/) documenta operaciones de objetos, pero **no** confirma que esas llamadas STS funcionen con Fluss. Prueba por separado las escrituras remotas del servidor, los tokens para clientes, los snapshots KV y la recuperación entre servidores. Mientras tanto, este es un ejemplo para explorar el esquema, no una configuración de producción. Consulta [almacenamiento remoto y credenciales](../api/remote-storage.md).
