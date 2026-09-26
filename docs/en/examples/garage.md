# Garage

**Kubernetes Secret · custom S3 endpoint · experimental compatibility**

Garage supports S3 access keys and path-style object requests. This manifest captures those settings in the current `FlussCluster` API; it is **not a claim of full Fluss compatibility**.

```yaml
{{#include garage.yaml}}
```

Provide the `data` namespace, a Garage bucket, and a Secret called `fluss-garage` in that namespace with `access-key` and `secret-key` entries. Configure Garage to grant that key access to the bucket and set `region` to Garage's actual configured region (`garage` is its documented default). Use the Kubernetes-reachable Garage S3 API endpoint, not a browser or admin endpoint. JVM settings, listeners, PDB and `scaleIn: Block` describe intended behavior only.

## Known unknown: temporary credentials

The manifest deliberately omits `delegation`. Fluss 1.0 uses STS `GetSessionToken` or `AssumeRole` to issue credentials to clients accessing remote data. Garage's [S3 compatibility matrix](https://garagehq.deuxfleurs.fr/documentation/reference-manual/s3-compatibility/) documents core object operations, but does **not** establish that those STS calls work with Fluss. Test the Fluss server's remote writes **and** the client-token, KV snapshot, and cross-server recovery paths independently. Until then this is a schema example for exploration, not a production configuration. See [Remote storage and credentials](../api/remote-storage.md).
