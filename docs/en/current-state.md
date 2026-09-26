# What works today

The `FlussCluster` schema is defined in `operator/src/api.rs`. It is a **proposed deployment contract**, not a statement that all the behavior described by the contract already runs. Documentation in this book follows the Rust types; Fluss server capabilities are called out separately.

| Capability | Current state |
| --- | --- |
| Define and serialize `FlussCluster` | Implemented in Rust; the schema has changed since the CRD was first installed in the development cluster. |
| Observe custom resources | `src/main.rs` watches `FlussCluster` in `operator-dev` and prints names and versions. |
| Watch all namespaces | Not implemented; the current process is scoped to `operator-dev`. |
| Create CoordinatorServers or TabletServers | Not implemented. The existing Fluss lab is managed by Helm, not this Operator. |
| Configure S3 or mount credential Secrets | Represented in the API only; no workload renders these fields yet. |
| Populate `.status` | The Rust status type exists; no status controller writes to Kubernetes. |
| Safe restart, upgrade, scale-in, or recovery | Not implemented. No spec update should be assumed to perform these operations. |

## Using the examples

The example manifests are **API examples**, not installation instructions. Their fields are valid according to the Rust data model, but the installed CRD may still contain an older schema, and applying a resource does not deploy a Fluss cluster until reconciliation exists. Avoid treating Kubernetes admission as a storage or IAM compatibility test.

The [EKS](examples/aws-eks.md), [MinIO](examples/minio.md), and [Garage](examples/garage.md) pages explain their different credential and delegation assumptions. In particular, the presence of an S3 URI does not prove that Fluss can obtain delegation tokens or recover a KV tablet on another server.

## Local documentation workflow

From the `operator/` directory:

```sh
./docs/build-docs.sh serve
```

Open `http://localhost:3000/en/` or `http://localhost:3000/es/` (override the port with `DOCS_PORT`). Both languages are served by the same server. `mdbook serve` only serves **one** language and its `/es/` URL will return 404. The generated HTML lives in `target/book/en/` and `target/book/es/`. Source pages are in `docs/en/` and `docs/es/`, with shared manifests in `docs/en/examples/*.yaml`.
