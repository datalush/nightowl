# Installing the operator

No Rust toolchain needed. Everything applies with plain `kubectl` from the
checked-in manifests in `deploy/`:

```bash
kubectl apply -f deploy/crd.yaml
kubectl apply -f deploy/serviceaccount.yaml
kubectl apply -f deploy/clusterrole.yaml
kubectl apply -f deploy/clusterrolebinding.yaml
kubectl apply -f deploy/deployment.yaml
```

The CRD manifest is generated from `src/api.rs` (`cargo run --bin gen-crd`)
and a unit test fails if it drifts, so reinstalling after an upgrade is the
same four commands with fresh files. Set `image:` in `deployment.yaml` to a
published build; the operator watches all namespaces by default (pass
`--namespace <name>` to pin one).

A minimal cluster, after creating the namespace and any S3 Secret it
references:

```bash
kubectl apply -f docs/en/examples/minio.yaml
kubectl get flussclusters -A
```

See [FlussCluster](api/fluss-cluster.md) for the full spec and
[Status and conditions](api/status.md) for what to watch while it converges.
