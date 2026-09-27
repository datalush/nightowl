# Night Owl — Kubernetes operator for Apache Fluss

Declarative Fluss clusters on Kubernetes: you describe a `FlussCluster`,
the controller converges Coordinators, TabletServers, config, storage,
budgets and health — refusing unsafe operations instead of guessing.

- CRD-first: versioned `FlussCluster` API, validated at admission and at
  reconcile with fail-closed guardrails.
- Evidence-based status: every condition carries reason, message and
  evidence; absent data stays absent, never invented.
- Safe by default: ordinal tablet identity, retained PVCs, protective
  PDBs, no auto-deletes, no foreign adoptions.
- Live config without restarts, Prometheus reporter on by default,
  optional Gateway (Deployment, Service, Ingress).

## Status

v1alpha1, verified live in k3d against Fluss 1.0.0. What works and what
doesn't: `docs/en/current-state.md`.

## Examples

Start from the manifests in `docs/en/examples/` (each carries its
verification status): [RustFS lab](docs/en/examples/rustfs.md) for
Secret-based S3, [AWS EKS](docs/en/examples/aws-eks.md) for workload
identity. Lab recreation, reset and versions: `docs/en/api/lab.md`.

## Contributing

See `CONTRIBUTING.md`. Full reference (EN/ES): `docs/en/index.md`.

## License

AGPL-3.0-only. See `LICENSE`.
