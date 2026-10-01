# Acceso nativo externo

Night Owl publica el protocolo nativo de Fluss detrás de una IP y un puerto
TCP de Envoy Gateway. Los clientes Java y Rust necesitan TLS/SNI. Fluss sigue
autenticando mediante SASL y aplicando sus ACL; Envoy no suplanta usuarios ni
interpreta peticiones Fluss.

## Configuración

Instalar Envoy Gateway, GatewayClass y CRDs compatibles con `Gateway/v1` y
`TLSRoute/v1`. Proporcionar una IP alcanzable y DNS para `fluss.example.com`
**y** `*.fluss.example.com`, ambos apuntando a esa misma IP. Crear un Secret
TLS en el namespace del FlussCluster; su certificado debe incluir el dominio
base y el wildcard y los clientes deben confiar en la CA. La plataforma
proporciona GatewayClass, IP y DNS público; el operador gestiona Gateway,
TLSRoutes, Services y sidecars del clúster.

```yaml
spec:
  listeners:
    external:
      domain: fluss.example.com
      gateway: {className: eg}
      tls: {secretName: fluss-external-tls}
  security:
    saslPlain:
      credentialsSecretName: fluss-native-users
      adminUser: admin
```

INTERNAL:9123 y CLIENT:9124 son los defaults; EXTERNAL escucha por defecto
en el puerto 9125 de **loopback del pod**. El sidecar escucha en 8443 y el
Service publica 443. `name`, `port` y `publicPort` son ajustes avanzados.
Los CR existentes sin `external` no adquieren una identidad pública inventada.
Para nuevos despliegues públicos hacen falta dominio, clase y Secret TLS.
Una vez activado, no puede retirarse `external` de un FlussCluster en marcha:
retirar el clúster por su ciclo de vida normal evita rutas públicas obsoletas.

Crear las credenciales SASL fuera de Git y del CR:

```bash
kubectl -n NAMESPACE create secret generic fluss-native-users \
  --from-file=credentials=/ruta/segura/credentials
```

El fichero contiene `usuario:contraseña,usuario:contraseña` e incluye
`adminUser`. Se monta solo en el contenedor Fluss mediante el proveedor
`${directory:...}`. CLIENT y EXTERNAL usan SASL/PLAIN; INTERNAL sigue en
PLAINTEXT. El authorizer nativo está activo y `User:<adminUser>` administra
las ACL. El Admin del operador utiliza INTERNAL y los endpoints de todos los
coordinadores, sin conocer las contraseñas de usuarios públicos. Una
NetworkPolicy propiedad del operador limita INTERNAL/CLIENT a pods Fluss y al
operador; solo se admite tráfico general en el puerto TLS del sidecar. No
instalar políticas adicionales que permitan saltarse ese aislamiento.
Restringir quién puede crear pods en este namespace: las etiquetas de una
NetworkPolicy no son una identidad. No puede habilitarse el Gateway HTTP junto
con las ACL nativas por usuario: no propaga las credenciales del llamante.

**La rotación de credenciales SASL aún no es automática.** Fluss carga el
Secret en el autenticador al arrancar: actualizarlo no revoca contraseñas en
procesos o conexiones existentes. Planificar un reinicio controlado de cada
servidor y comprobar las credenciales nuevas y la revocación desde fuera.
La rotación del certificado TLS es distinta: usa SDS sin reiniciar pods.

## Routing y descubrimiento

Ejemplo `analytics` con dos coordinadores y tres tablets:

| Endpoint público | Service destino |
| --- | --- |
| `fluss.example.com:443` | `analytics-bootstrap` (tablets Ready) |
| `coordinator-0.fluss.example.com:443` | `analytics-coordinator-0-external` |
| `coordinator-1.fluss.example.com:443` | `analytics-coordinator-1-external` |
| `tablet-0.fluss.example.com:443` | `analytics-tabletserver-0-external` |
| `tablet-1.fluss.example.com:443` | `analytics-tabletserver-1-external` |
| `tablet-2.fluss.example.com:443` | `analytics-tabletserver-2-external` |

Envoy enruta por SNI sin terminar TLS. Claves y certificados residen en el
sidecar del pod correspondiente. Fluss descubre coordinador activo y tablets;
los clientes configuran solo el bootstrap base con TLS/SASL, aplicado también
a las conexiones descubiertas. Un standby como único bootstrap rechaza
metadatos: el Service bootstrap selecciona tablets. Escalar añade rutas
y Services sin cambiar endpoints previos. El EXTERNAL sin cifrar solo acepta
conexiones desde el loopback de su pod.
Al reducir réplicas, Fluss debe demostrar primero que los tablets retirados
no alojan réplicas. Solo cuando el StatefulSet haya reducido sus réplicas y
desaparezca el pod, el operador borra su TLSRoute y Service **propios**. Nunca
borra los datos del PVC.

El sidecar utiliza SDS de ficheros proyectados desde el Secret. Actualizar
`tls.crt` y `tls.key` cambia el certificado sin reiniciar Fluss (verificado
con Envoy v1.33.4). Rotar la CA requiere actualizar también la confianza
del cliente.

`status.externalEndpoints` informa de Services convergidos, **no** de
accesibilidad pública. `NativeRoutesProgrammed=True` indica Gateway programado
y rutas aceptadas/resueltas con generación reciente; no prueba DNS, firewall
ni una conexión TLS real.

## DNS dividido opcional

Solo activar `listeners.external.dns.internalMapping: true` si la plataforma
integra CoreDNS. El operador genera un ConfigMap `<cluster>-native-dns` con
reglas exactas `rewrite.override` hacia Services, conservando **los mismos
hostnames públicos**. La plataforma debe instalar ese fragmento en CoreDNS y
recargarlo; el operador no modifica su configuración global. En k3s se puede
integrar con `kube-system/coredns-custom` (`*.override`). Al crear por primera
vez el ConfigMap opcional puede hacer falta reiniciar CoreDNS para montar el
volumen. Desactivar el flag retira el ConfigMap propio; la plataforma debe
retirar su fragmento instalado. Sin mapping, clientes internos pueden usar
la ruta pública si es accesible o la configuración privada CLIENT.

## Verificación

Probar Java y Rust desde fuera con bootstrap único `fluss.example.com:443`,
TLS y SASL: escribir y leer a través de varios tablets, permitir READ a Alice
y denegar a Bob, reiniciar un tablet conservando PVC, cambiar el líder y
escalar. Incluir workers Spark/Flink, no solo el driver. Las lecturas remotas
pueden requerir acceso adicional a S3: TLS RPC no proxifica ese tráfico.
Consultar [evidencia de laboratorio](lab.md) para el alcance demostrado. El
anterior diseño por puertos y TCPRoutes fue experimental; no es la API actual.
