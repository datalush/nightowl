<div class="cover">
  <span class="eyebrow">DATALUSH / OPERATOR DOCUMENTATION</span>
  <h1>Operate Fluss<br>with intent.</h1>
  <p>A declarative Kubernetes API for Apache Fluss clusters. Explore the resource contract, inspect complete manifests, and follow implementation progress without mistaking an API design for a finished controller.</p>
  <span class="cover-meta">FlussCluster · fluss.datalush.com/v1alpha1 · Fluss 1.0.0 baseline</span>
</div>

## Start here

<div class="doc-grid">
  <a class="doc-card" href="api/fluss-cluster.html"><span class="card-index">01 / THE CONTRACT</span><strong>FlussCluster API</strong><span>Fields, types, constraints, and resource ownership.</span><span class="card-arrow">Explore →</span></a>
  <a class="doc-card" href="api/remote-storage.html"><span class="card-index">02 / REMOTE STORAGE</span><strong>S3 and credentials</strong><span>Workload identity, Secrets, and delegation tokens.</span><span class="card-arrow">Explore →</span></a>
  <a class="doc-card" href="current-state.html"><span class="card-index">03 / IMPLEMENTATION</span><strong>What works today</strong><span>A clear boundary between the schema and the controller.</span><span class="card-arrow">Explore →</span></a>
</div>

## Choose an environment

| Environment | Credential source | Example |
| --- | --- | --- |
| AWS EKS | Existing ServiceAccount using IRSA or EKS Pod Identity | [EKS manifest](examples/aws-eks.md) |
| MinIO | Kubernetes Secret with access and secret keys | [MinIO manifest](examples/minio.md) |
| Garage | Kubernetes Secret; STS compatibility remains unverified | [Garage manifest](examples/garage.md) |

> **Documentation scope.** The manifests illustrate the current Rust API in `operator/src/api.rs`; this project does not yet create Fluss resources from a `FlussCluster`. See [What works today](current-state.md) before applying an example.

## Design principles

- **Kubernetes owns intent.** A `FlussCluster` is namespaced; its managed resources will live in that namespace.
- **Remote storage is explicit.** TabletServer PVCs and shared S3 storage solve different problems.
- **Secrets remain references.** Credential values do not belong in a custom resource or a ConfigMap.
- **Unsafe changes do not silently proceed.** Upgrades and scale-in require Fluss-aware safety checks.

The repository's `ROADMAP.md` describes the wider engineering direction; this book documents the Operator's concrete API and its implementation status.
