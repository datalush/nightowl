# RustFS

**Verified live · Kubernetes Secret · custom S3 endpoint · laboratory**

This manifest configures remote storage for a RustFS backend. The operator has been exercised with RustFS 1.0.0: tablets wrote KV snapshots and the `AssumeRole` flow issued usable client credentials. `RemoteStorageReady=True` only checks references; it does not check those operations. The endpoint below is a placeholder. Supply your own reachable backend, bucket and IAM user credentials before applying a copy.

```yaml
{{#include rustfs.yaml}}
```

Create the `data` namespace and an existing Secret named `fluss-rustfs` there with keys `access-key` and `secret-key`. The API contains **references**, not their values: the operator mounts the Secret read-only into the pods and renders `${directory:...}` markers into `server.yaml`.

Fluss performs recovery from snapshots and surviving replicas; the operator reports the observed health. See [remote storage and credentials](../api/remote-storage.md) for the tested scope and limits.
