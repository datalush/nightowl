# What works today

The `FlussCluster` schema is defined in `operator/src/api.rs` and the controller converges it into running Fluss clusters. Capabilities below are verified live in k3d unless marked otherwise.

| Capability | Current state |
| --- | --- |
| Define and validate `FlussCluster` | Implemented; CEL rules enforced live, checked-in CRD tested for drift. |
| Watch all namespaces (or one via flag) | Implemented; same-name clusters isolated per namespace, verified. |
| Run CoordinatorServers and TabletServers | Implemented; per-ordinal identity, staged config, hash-pinned rollouts. |
| Configure S3 and mount credential Secrets | Implemented and verified against RustFS (KV snapshots land in the bucket). |
| Least-privilege RBAC | Implemented; verified under its own ServiceAccount with zero `Forbidden`. |
| Populate `.status` | Implemented: resources, storage, reachability and cluster health with evidence. |
| JVM heap, PVCs, PDBs, scheduling, client Service | Implemented; oversized heaps blocked before pods converge. |
| Safe restart, upgrade, scale-in, recovery | Partial: restarts and recovery verified; controlled upgrade/scale-in pending. |
| Cross-server restore, client tokens, real AWS | Not verified yet; tracked separately. |

## Using the examples

The example manifests deploy real clusters; each carries its verification status. The [EKS](examples/aws-eks.md) and [RustFS](examples/rustfs.md) pages explain their different credential and delegation assumptions. In particular, the presence of an S3 URI does not prove that Fluss can obtain delegation tokens or recover a KV tablet on another server.

## Local documentation workflow

From the `operator/` directory:

```sh
./docs/build-docs.sh serve
```

Open `http://localhost:3000/en/` or `http://localhost:3000/es/` (override the port with `DOCS_PORT`). Both languages are served by the same server. `mdbook serve` only serves **one** language and its `/es/` URL will return 404. The generated HTML lives in `target/book/en/` and `target/book/es/`. Source pages are in `docs/en/` and `docs/es/`, with shared manifests in `docs/en/examples/*.yaml`.
