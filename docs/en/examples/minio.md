# MinIO

**Kubernetes Secret · custom S3 endpoint · local laboratory**

This manifest expresses Fluss remote storage through a MinIO S3 endpoint. The hostname, bucket, and credentials are examples: provide an actual MinIO Service and bucket before testing. The Operator does not provision MinIO.

```yaml
{{#include minio.yaml}}
```

Create the `data` namespace and an existing Secret named `fluss-minio` there with keys `access-key` and `secret-key`. The API contains **references**, not their values. The intended controller will mount the Secret read-only and render `${directory:...}` markers in Fluss configuration; it does not do so yet. The listener, PDB, JVM and `scaleIn: Block` fields in the manifest are likewise API intent, not implemented operations.

## Delegation remains a separate test

This example intentionally **omits `delegation`**. It describes server S3 settings, not a verified client-token workflow. Fluss 1.0 calls `GetSessionToken` for static keys unless configured for `AssumeRole`; a successful S3 write does not prove either STS path works against the chosen MinIO version. If that deployment supports the `AssumeRole` request Fluss sends, the schema can express it:

```yaml
delegation:
  type: assumeRole
  roleArn: <role-understood-by-your-STS>
  stsEndpoint: http://minio.storage.svc.cluster.local:9000
```

These are values **under `spec.remoteStorage.s3`**, not an extra top-level section. Verify token issuance, remote KV snapshots, recovery on another TabletServer, and remote reads before relying on this configuration for durable workloads. See [Remote storage and credentials](../api/remote-storage.md).
