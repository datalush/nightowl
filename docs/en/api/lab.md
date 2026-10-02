# Local lab

Use a k3d cluster to exercise the operator against Fluss 1.0.0. You need
`k3d`, `kubectl`, `helm`, a Rust toolchain and an S3-compatible backend reachable
from the cluster. The operator does not create ZooKeeper, buckets or credentials.
The [RustFS manifest](../examples/rustfs.md) uses a placeholder endpoint: replace
it with your own endpoint and bucket before applying it.

## Create the cluster

```bash
k3d cluster create lab --servers 1 --agents 3
kubectl create namespace fluss
kubectl create namespace data
helm repo add bitnami https://charts.bitnami.com/bitnami
helm install zk bitnami/zookeeper --namespace fluss --version 0.15.0
kubectl apply -f deploy/crd.yaml
```

Wait for `zk-zookeeper-0` in `fluss` to become Ready. The example connects to
`zk-zookeeper.fluss.svc.cluster.local:2181`. If your ZooKeeper release or
Service has another name, change `zookeeper.addresses` in the manifest.

Create the `fluss-lab` bucket on your S3 backend. Supply credentials for an
IAM user authorized to access it and to obtain RustFS `AssumeRole` sessions;
root and service-account keys do not support that flow. Keep credentials out
of Git. For example, with credential files already present on your machine:

```bash
kubectl -n data create secret generic fluss-rustfs \
  --from-file=access-key=/path/to/access-key \
  --from-file=secret-key=/path/to/secret-key
```

Edit a **local copy** of `docs/en/examples/rustfs.yaml`: replace the placeholder
S3 endpoint with an address reachable from the pods, and adjust the bucket and
prefix if necessary. Give each test cluster its own prefix. Then start the
operator and apply that copy from another terminal:

```bash
RUST_LOG=info cargo run --bin nightowl -- --namespace data
```

```bash
kubectl apply -f /path/to/local-rustfs.yaml
kubectl -n data get flussclusters,pods,pvc
kubectl -n data describe flusscluster rustfs-lab
```

When the API changes, reapply `deploy/crd.yaml` before starting the new
operator. An outdated CRD can reject new condition values or discard new
status fields. `RemoteStorageReady=True` confirms that the Secret reference
and its keys exist; it does **not** test S3 or STS connectivity. Check pod
logs and perform a write/read with a Fluss client to test the data path.

## Access from the host

Fluss advertises pod DNS names and IPs that may not resolve or route from
the host. Prefer running clients inside Kubernetes. For host-side native
client tests, use the [external TLS/SNI listener](native-external-access.md)
or arrange temporary host DNS and pod-network routing for your k3d setup;
remove those host changes after the test.

## Reset

Deleting a `FlussCluster` does not delete its PVC data or its S3 objects.
For a clean test, delete the CR, explicitly remove the PVCs belonging to
**that test cluster** if its data is no longer needed, and delete the
temporary Secret. Remove the test's S3 prefix separately if you want to
discard its remote data. To discard the entire local cluster, run
`k3d cluster delete lab`; that also removes ZooKeeper metadata and all
other workloads in the cluster.

## Versions to reproduce this setup

| Component | Version |
| --- | --- |
| Fluss server and example manifest | 1.0.0 |
| ZooKeeper Helm chart | Bitnami `0.15.0` |
| Tested k3d / k3s baseline | k3d `v5.9.0` / k3s `v1.35.5+k3s1` |

The public example has not been verified on AWS EKS. The RustFS setup has
been exercised with an external RustFS 1.0.0 instance; it is not provisioned
by these commands. For S3 authentication and recovery limits, see
[remote storage](remote-storage.md).
