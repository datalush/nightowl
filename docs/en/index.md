<div class="cover">
  <span class="eyebrow">DATALUSH / OPERATOR DOCUMENTATION</span>
  <h1>Run Fluss<br>on Kubernetes.</h1>
  <p>Configure Apache Fluss clusters with Night Owl. Start with an example, check the API reference, and see which integrations have been tested.</p>
  <span class="cover-meta">FlussCluster · fluss.datalush.com/v1alpha1 · Fluss 1.0.0 baseline · AGPL-3.0-only</span>
</div>

## Start here

<div class="doc-grid">
  <a class="doc-card" href="api/fluss-cluster.html"><span class="card-index">01 / THE CONTRACT</span><strong>FlussCluster API</strong><span>Fields, types, constraints, and resource ownership.</span><span class="card-arrow">Explore →</span></a>
  <a class="doc-card" href="api/remote-storage.html"><span class="card-index">02 / REMOTE STORAGE</span><strong>S3 and credentials</strong><span>Workload identity, Secrets, and delegation tokens.</span><span class="card-arrow">Explore →</span></a>
  <a class="doc-card" href="current-state.html"><span class="card-index">03 / STATUS</span><strong>What works today</strong><span>Implemented features and tested integrations.</span><span class="card-arrow">Explore →</span></a>
</div>

## Choose an environment

| Environment | Credential source | Example | Verification |
| --- | --- | --- | --- |
| RustFS lab | Kubernetes Secret with access and secret keys | [RustFS manifest](examples/rustfs.md) | Verified live (converge, KV snapshots, SigV4/AssumeRole) |
| AWS EKS | Existing ServiceAccount using IRSA or EKS Pod Identity | [EKS manifest](examples/aws-eks.md) | Planned |

The examples use the API in `src/api.rs`. See [what works today](current-state.md) for tested integrations before choosing a backend.

## How it works

- **One cluster per resource.** A `FlussCluster` and its managed resources live in the same namespace.
- **Remote storage is explicit.** TabletServer PVCs and shared S3 storage solve different problems.
- **Secrets remain references.** Credential values do not belong in a custom resource or a ConfigMap.
- **Unsafe changes do not silently proceed.** Upgrades and scale-in require Fluss-aware safety checks.

To try the operator locally, follow the [lab guide](api/lab.md).
