# What works today

The `FlussCluster` API is defined in `src/api.rs`. The operator has been exercised in k3d; backend-specific verification is noted below.

| Capability | Current state |
| --- | --- |
| Define and validate `FlussCluster` | Implemented; CEL rules enforced live, checked-in CRD tested for drift. |
| Watch all namespaces (or one via flag) | Implemented; same-name clusters isolated per namespace, verified. |
| Run CoordinatorServers and TabletServers | Implemented; per-ordinal identity, staged config, hash-pinned rollouts. |
| Configure S3 and mount credential Secrets | Implemented and verified against RustFS (KV snapshots land in the bucket). |
| Least-privilege RBAC | Implemented; verified under its own ServiceAccount with zero `Forbidden`. |
| Populate `.status` | Implemented: resources, storage, reachability and cluster health with evidence. |
| JVM heap, PVCs, PDBs, scheduling, client Service | Implemented; oversized heaps blocked before pods converge. |
| Restarts and scale-in | Sequenced restarts implemented; scale-in requires a fresh per-server read proving outgoing servers are registered and empty. Stock Fluss 1.0 lacks that read. No automatic rebalance. |
| Recovery and client tokens | Disk replacement and follower promotion exercised with RustFS; Fluss-issued S3 client tokens exercised with RustFS IAM user credentials. Recovery is performed by Fluss, not the operator. |
| AWS EKS | Manifest provided; IAM, client-token and recovery flows not tested on AWS. |

## Using the examples

The [EKS](examples/aws-eks.md) and [RustFS](examples/rustfs.md) examples have different credential requirements. Neither an S3 URI nor `RemoteStorageReady=True` proves that Fluss can issue client tokens or recover data. The RustFS example contains a placeholder endpoint: replace it before use.

## Local documentation workflow

From the `operator/` directory:

```sh
./docs/build-docs.sh serve
```

Open `http://localhost:3000/en/` or `http://localhost:3000/es/` (override the port with `DOCS_PORT`). Both languages are served by the same server. `mdbook serve` only serves **one** language and its `/es/` URL will return 404. The generated HTML lives in `target/book/en/` and `target/book/es/`. Source pages are in `docs/en/` and `docs/es/`, with shared manifests in `docs/en/examples/*.yaml`.
