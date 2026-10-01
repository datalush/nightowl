# Native external access

Night Owl publishes Fluss's native protocol behind a single Envoy Gateway IP
and TCP port. Java and Rust clients need TLS/SNI support. Fluss still performs
SASL authentication and enforces its own ACLs; Envoy does not impersonate users
or parse Fluss requests.

## Configure

Install Envoy Gateway, a GatewayClass and Gateway API CRDs supporting
`Gateway/v1` and `TLSRoute/v1`. Provide a reachable IP and DNS records for both
`fluss.example.com` and `*.fluss.example.com` pointing to that **same IP**.
Provision a TLS Secret in the FlussCluster namespace with a certificate valid
for the base hostname **and** the wildcard. Its CA must be trusted by clients.
The platform owns the GatewayClass, IP and public DNS; Night Owl owns the
per-cluster Gateway, TLSRoutes, Services and TLS sidecars.

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

`listeners.internal` and `listeners.client` default to INTERNAL:9123 and
CLIENT:9124; the native EXTERNAL listener defaults to port 9125 on **pod
loopback only**. The generated sidecar listens on 8443; its Service exposes
443. `name`, `port`, and `publicPort` inside `listeners.external` are optional
advanced overrides. Existing CRs without `external` remain unchanged rather
than receiving an invented public identity. New public deployments require the
domain, class and TLS Secret above.
Once enabled, external access cannot be removed from a running FlussCluster;
retire the cluster through its normal lifecycle rather than leaving stale
public routes pointing at an old listener.

Create the SASL Secret imperatively; never put passwords in the CR or Git:

```bash
kubectl -n NAMESPACE create secret generic fluss-native-users \
  --from-file=credentials=/secure/location/credentials
```

The file contains Fluss's `username:password,username:password` map. It must
include `adminUser`. The operator mounts it only into the Fluss container and
resolves `${directory:...}` at server startup. When enabled, it configures
CLIENT/EXTERNAL with SASL/PLAIN, INTERNAL with PLAINTEXT, turns on Fluss's
authorizer and provisions `User:<adminUser>` as superuser for ACL administration.
The operator's own Admin traffic uses INTERNAL and all coordinator addresses;
it does not store or use the public users' passwords. Do not add permissive
NetworkPolicies targeting these pods: an owned ingress policy restricts the
plaintext INTERNAL/CLIENT listeners to Fluss pods and the operator. Only the
sidecar TLS port accepts other inbound traffic.
Restrict who can create pods in the Fluss namespace: NetworkPolicy selectors
are labels, not an identity mechanism. The optional HTTP Gateway cannot be
enabled alongside native per-user ACL security; it does not forward user
credentials to Fluss.

**SASL credential rotation is not yet automatic.** Fluss reads the mounted
credentials into its in-memory authenticator at startup; updating the Secret
does not revoke credentials on existing processes or connections. Plan a
controlled per-server restart when changing that Secret, and confirm the new
credentials and revocation from an external client. TLS certificate rotation
is separate and uses SDS without restarting pods.

## Routing and discovery

For `metadata.name: analytics`, two coordinators and three tablets:

| Public endpoint | Owned backend |
| --- | --- |
| `fluss.example.com:443` | `analytics-bootstrap` (Ready tablets) |
| `coordinator-0.fluss.example.com:443` | `analytics-coordinator-0-external` |
| `coordinator-1.fluss.example.com:443` | `analytics-coordinator-1-external` |
| `tablet-0.fluss.example.com:443` | `analytics-tabletserver-0-external` |
| `tablet-1.fluss.example.com:443` | `analytics-tabletserver-1-external` |
| `tablet-2.fluss.example.com:443` | `analytics-tabletserver-2-external` |

Envoy routes by SNI in TLS **passthrough** mode, so certificates and private
keys remain inside the destination pod's sidecar. Fluss discovers the active
coordinator and tablets on its own. Clients configure only the base bootstrap
and the same TLS/SASL settings for every discovered connection. A coordinator
standby used as the only bootstrap refuses metadata; the tested bootstrap
Service therefore selects tablets. Scale-out creates routes and Services for
new ordinals without changing existing addresses. NetworkPolicies keep Fluss's
unencrypted EXTERNAL listener unreachable outside its own pod.
On scale-in, Fluss must first prove outgoing tablets hold no replicas. Only
after the StatefulSet has shrunk and the retired pod has disappeared does the
operator delete its **owned** TLSRoute and Service. PVC data is never deleted.

The sidecar uses filesystem SDS with a mounted Secret. Updating `tls.crt` and
`tls.key` atomically through Kubernetes Secret projection reloads the
certificate without restarting a Fluss pod (verified with Envoy v1.33.4).
When rotating to a different CA, the client must also trust the new CA.

`status.externalEndpoints` describes Services converged by this operator,
**not** proven public reachability. `NativeRoutesProgrammed=True` means the
Gateway is programmed and the TLSRoutes have fresh Accepted/ResolvedRefs
conditions, not that DNS, firewall or a TLS client has been tested. Verify a
real client from outside Kubernetes.

## Optional split DNS for in-cluster clients

Set `listeners.external.dns.internalMapping: true` only if your platform
provides the CoreDNS integration. The operator creates an owned
`<cluster>-native-dns` ConfigMap containing exact CoreDNS `rewrite.override`
rules mapping the **same public hostnames** to per-pod Services. The platform
must make that fragment available to CoreDNS and reload it; the operator
never modifies cluster-wide DNS. On k3s, the platform can merge it into
`kube-system/coredns-custom`, whose `*.override` files are imported by CoreDNS.
The first creation of an optional `coredns-custom` ConfigMap may require a
CoreDNS rollout before its volume is mounted. Disabling the flag removes the
owned mapping; the platform must also remove its installed fragment. With the
mapping disabled, clients within Kubernetes may use the public route if
reachable, or use existing private CLIENT settings.

## Verification

Test both Java and Rust clients with TLS and SASL via a single
`fluss.example.com:443` bootstrap: write and read data on multiple tablets,
grant Alice READ, deny Bob, restart a tablet preserving its PVC, change
coordinator leadership and scale out. Include Spark/Flink workers, not only
the driver. Remote snapshots and log reads may also need direct access to
object storage; RPC TLS does not proxy S3. See [lab evidence](lab.md) for what
has actually been exercised. The operator's earlier per-port TCPRoute design
was an experimental phase and is not the current configuration.
