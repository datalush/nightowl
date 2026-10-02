# Installing the operator

No Rust toolchain needed. Everything applies with plain `kubectl` from the
checked-in manifests in `deploy/`:

```bash
kubectl apply -f deploy/crd.yaml
kubectl create namespace operator-system
kubectl apply -f deploy/serviceaccount.yaml
kubectl apply -f deploy/clusterrole.yaml
kubectl apply -f deploy/clusterrolebinding.yaml
kubectl apply -f deploy/deployment.yaml
```

The CRD manifest is generated from `src/api.rs` (`cargo run --bin gen-crd`).
Apply the updated manifests when upgrading the operator. Before applying the
Deployment, set its `image:` to a published build. The operator watches all
namespaces by default; pass `--namespace <name>` to watch one namespace.

A sample cluster, after creating the `data` namespace, its S3 Secret and
an S3 bucket: copy the RustFS manifest and replace its placeholder endpoint
with an address reachable from the pods. Then apply your local copy:

```bash
kubectl apply -f /path/to/local-rustfs.yaml
kubectl get flussclusters -A
```

See [FlussCluster](api/fluss-cluster.md) for the full spec and
[Status and conditions](api/status.md) for what to watch while it converges.
