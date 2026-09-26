# RustFS

**Verified live · Kubernetes Secret · custom S3 endpoint · laboratory**

This manifest expresses Fluss remote storage through the lab RustFS backend. It is the only S3 example verified end to end: the operator converges it with `RemoteStorageReady=True`, tablets write KV snapshots under the prefix, and SigV4 plus `AssumeRole` credential flows were proven against RustFS 1.0.0. Bucket names, prefixes and credentials below are the lab's; provide your own backend before reusing the shape.

```yaml
{{#include rustfs.yaml}}
```

Create the `data` namespace and an existing Secret named `fluss-rustfs` there with keys `access-key` and `secret-key`. The API contains **references**, not their values: the operator mounts the Secret read-only into the pods and renders `${directory:...}` markers into `server.yaml`.

Cross-server restore and client token issuance are tracked separately; a successful S3 write does not prove either workflow. See [Remote storage and credentials](../api/remote-storage.md).
