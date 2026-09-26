# Remote storage and credentials

Fluss uses local TabletServer disks for hot data and **shared remote storage** for KV snapshots and tiered log segments. These are separate storage layers. In distributed mode, a local filesystem path on each pod cannot substitute for a shared S3 location. The current API accepts one S3-shaped location under `spec.remoteStorage.s3`.

## S3 location

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `bucket` | string | Yes | Existing bucket; the Operator does not provision it. |
| `prefix` | string | Yes | Dedicated key prefix for this FlussCluster. Do not share it with another cluster. |
| `region` | string | Yes | Region passed to the Fluss S3 filesystem plugin. |
| `endpoint` | URL | No | Custom S3-compatible endpoint, typically MinIO or Garage. |
| `pathStyleAccess` | boolean | No | Defaults to `false`; set to `true` for local S3-compatible endpoints. |
| `authentication` | tagged object | Yes | Either `workloadIdentity` or `secret`. |
| `delegation` | tagged object | No | `assumeRole` or `getSessionToken`; see the distinction below. |

The future reconciler will render `s3://<bucket>/<prefix>` as a Fluss remote location and pass `s3.region`, `s3.endpoint`, and `s3.path-style-access` where applicable. Fluss 1.0 recommends `remote.data.dirs` for new clusters, even with a single location. The location should be treated as immutable after data has been written; changing it requires a migration, not merely a new pod configuration.

## Server authentication

### Workload identity on EKS

```yaml
authentication:
  type: workloadIdentity
  serviceAccountName: fluss-production
delegation:
  type: assumeRole
  roleArn: arn:aws:iam::123456789012:role/fluss-clients-read
```

`serviceAccountName` is an **existing ServiceAccount in the FlussCluster namespace**. The AWS administrator associates it with an IAM role via IRSA or EKS Pod Identity; this Operator does not create an IAM role or an EKS Pod Identity association. The Fluss pods use the AWS SDK's default credential chain without `s3.access-key` or `s3.secret-key`.

Concretely, with `workloadIdentity` the renderer emits **no credential keys at all** — no `s3.access-key`, no `s3.secret-key`, no `config.providers` block. The only identity-related key comes from `delegation` (`s3.assumed.role.arn`, required in this mode).

The ServiceAccount's IAM role and `delegation.roleArn` are **not the same setting**. The first identifies the Fluss server, which needs S3 access and `sts:AssumeRole` permission. The second is the role Fluss assumes to issue temporary credentials to clients. Fluss 1.0 requires an assumed role when server credentials come from the default AWS chain. EKS Pod Identity uses the SDK container credential provider; integration with Fluss must still be exercised in EKS before claiming a tested deployment.

### Existing Kubernetes Secret

```yaml
authentication:
  type: secret
  secretRef:
    name: fluss-s3
    accessKeyKey: access-key
    secretKeyKey: secret-key
delegation:
  type: getSessionToken
```

The Secret must exist in the **same namespace** as the FlussCluster. The intended workload mounts it as read-only files and renders Fluss configuration markers, not credential values, into the ConfigMap:

```yaml
config.providers: directory
config.providers.directory.param.allowed.paths: /etc/fluss/secrets
s3.access-key: ${directory:/etc/fluss/secrets/s3:access-key}
s3.secret-key: ${directory:/etc/fluss/secrets/s3:secret-key}
```

Fluss resolves these markers at startup, so rotating a Secret requires restarting the relevant servers. This mount and restart behavior is **not implemented yet**.

## Delegation is a separate compatibility requirement

The server's ability to read and write S3 objects does not imply that Fluss can issue credentials to Flink/Spark clients reading remote data. Fluss 1.0's S3 delegation-token provider calls **`GetSessionToken`** with static keys by default, or **`AssumeRole`** when given a role ARN. With workload identity, `AssumeRole` is required. For S3-compatible services, `assumeRole` also accepts an optional `stsEndpoint` pointing at that service's STS endpoint.

| Backend | What the API expresses | What must be verified |
| --- | --- | --- |
| AWS S3 | EKS workload identity with `assumeRole`; static keys with a selected STS mode. | IAM trust and permissions, client-token flow, snapshot/restore, and failover. |
| MinIO | Secret, custom endpoint, path-style, chosen STS mode. | The **specific MinIO version/configuration** must accept the STS request Fluss sends and the returned credentials. |
| RustFS | Secret, custom endpoint, path-style, AssumeRole. | Verified 2026-09-26 against the lab instance: bucket round-trip (put/get/list/delete) from cluster pods, and `AssumeRole` returns temporary credentials that authorize S3 operations. KV snapshots and Fluss-issued client tokens still need a Fluss runtime pointed at it. |
| Garage | Secret, custom endpoint, path-style. | Garage's documented S3 operations do not establish support for Fluss's STS delegation flow. Do not infer full KV/client compatibility. |

`delegation` is optional with static keys; omission does **not** assert that the backend supports token issuance. The generated CRD now includes CEL rules requiring `assumeRole` with `workloadIdentity` and matching `secretRef` / `serviceAccountName` / `roleArn` to their selected `type`. An older installed CRD will not contain those rules until updated. The reconciler verifies the actual Secret (existence plus referenced keys) and ServiceAccount before reporting `RemoteStorageReady`, refusing with evidence naming the missing object otherwise; a Secret or ServiceAccount appearing later does not retrigger reconciliation on its own — the next watched event does. This needs `get` on Secrets and ServiceAccounts in the cluster namespace (real-deployment RBAC, tracked under 5vrz). Basic S3 operations are verified against lab RustFS; KV snapshots and Fluss-issued client token flow still need a Fluss runtime pointed at a backend. The examples are API manifests, not compatibility certifications.

Sources: [Fluss 1.0 S3 configuration](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/), [Fluss 1.0 secret providers](https://fluss.apache.org/docs/security/secrets/), [EKS IRSA](https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html), [EKS Pod Identity](https://docs.aws.amazon.com/eks/latest/userguide/pod-identities.html), [Garage's S3 compatibility matrix](https://garagehq.deuxfleurs.fr/documentation/reference-manual/s3-compatibility/), and [RustFS STS documentation](https://docs.rustfs.com/en/security-compliance/iam/sts).
