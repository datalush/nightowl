# Remote storage and credentials

Fluss uses local TabletServer disks for hot data and **shared remote storage** for KV snapshots and tiered log segments. These are separate storage layers. In distributed mode, a local filesystem path on each pod cannot substitute for a shared S3 location. The current API accepts one S3-shaped location under `spec.remoteStorage.s3`.

## S3 location

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `provider` | `aws` or `rustfs` | No | Preset only. Omitted means `aws`; `provider: aws` is equivalent. Explicit fields always win. |
| `bucket` | string | Yes | Existing bucket; the Operator does not provision it. |
| `prefix` | string | Yes | Dedicated key prefix for this FlussCluster. Do not share it with another cluster. |
| `region` | string | Yes | Region passed to the Fluss S3 filesystem plugin. |
| `endpoint` | URL | No | Custom S3-compatible endpoint, e.g. the lab RustFS. |
| `authentication` | tagged object | Yes | Either `workloadIdentity` or `secret`. |
| `delegation` | object | No | `roleArn` implies `AssumeRole`; optional `type` overrides the mode, `stsEndpoint` overrides the STS address. |

Omitting `provider` selects AWS defaults. Path-style S3 addressing is always
on. A custom endpoint does not select the RustFS preset: configure its
delegation and STS endpoint explicitly.

AWS and custom endpoints require a real `delegation.roleArn` for the default
`AssumeRole` mode. Use `delegation.type: getSessionToken` only when the backend
supports it. Provider and delegation changes require a restart; changing
bucket or prefix requires a data migration.

The reconciler renders `s3://<bucket>/<prefix>` into the singular `remote.data.dir` and passes `s3.region`, `s3.endpoint`, and `s3.path-style-access` where applicable. The singular key is deliberate: the `apache/fluss:1.0.0` image ignores the plural `remote.data.dirs` and fails startup on a null remote path (see ADR-0001). The location should be treated as immutable after data has been written; changing it requires a migration, not merely a new pod configuration.

## Server authentication

### Workload identity on EKS

```yaml
authentication:
  type: workloadIdentity
  serviceAccountName: fluss-production
delegation:
  roleArn: arn:aws:iam::123456789012:role/fluss-clients-read
```

`serviceAccountName` is an **existing ServiceAccount in the FlussCluster namespace**. The AWS administrator associates it with an IAM role via IRSA or EKS Pod Identity; this Operator does not create an IAM role or an EKS Pod Identity association. The Fluss pods use the AWS SDK's default credential chain without `s3.access-key` or `s3.secret-key`.

Concretely, with `workloadIdentity` the renderer emits **no credential keys at all** — no `s3.access-key`, no `s3.secret-key`, no `config.providers` block. The only identity-related key comes from `delegation` (`s3.assumed.role.arn`, required in this mode).

The ServiceAccount's IAM role identifies the Fluss server; it needs S3 access
and permission to assume the client role. `delegation.roleArn` identifies
**that client role**, used to issue temporary credentials. They are different
roles. A role ARN selects `AssumeRole` without an explicit `type`.

Fluss 1.0 requires the assumed role when server credentials come from the
default AWS chain. EKS Pod Identity uses the SDK's container credential
provider; its integration with Fluss has not been tested in EKS.

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

The Secret must exist in the **same namespace** as the FlussCluster. The workload mounts it as read-only files and renders Fluss configuration markers, not credential values, into the ConfigMap:

```yaml
config.providers: directory
config.providers.directory.param.allowed.paths: /etc/fluss/secrets
s3.access-key: ${directory:/etc/fluss/secrets/s3:access-key}
s3.secret-key: ${directory:/etc/fluss/secrets/s3:secret-key}
```

Fluss resolves the markers at startup, so a rotated Secret requires server
restarts. The operator detects a mismatch between the Secret and the hash
pinned to each pod and reports `S3CredentialsStale=True`; it does not restart
pods just because the Secret changed. AWS static-key users also need a role
unless they explicitly select `getSessionToken`.

### RustFS preset

```yaml
provider: rustfs
bucket: fluss
prefix: clusters/dev
region: us-east-1
endpoint: https://storage.example.com
authentication:
  type: secret
  secretRef: {name: fluss-rustfs, accessKeyKey: access-key, secretKeyKey: secret-key}
```

The operator always emits path-style access; the RustFS preset adds `AssumeRole` and an STS endpoint equal to
`endpoint`. RustFS 1.0 accepts the preset's conventional RoleArn for AWS
compatibility; it grants **no** permissions by itself. Supply long-term RustFS
**IAM user** keys with a policy permitting access to the bucket. Root and
service-account keys cannot call RustFS `AssumeRole`. Override
`delegation.roleArn` or `delegation.stsEndpoint` when needed. The operator checks the Secret keys but
does **not** verify STS issuance; `RemoteStorageReady=True` alone is not a
client-token readiness claim.

## Delegation is a separate compatibility requirement

An S3 write by a server does not prove that Flink or Spark clients can obtain
temporary credentials for remote reads. For AWS and custom endpoints, the
operator defaults to `AssumeRole` with a real `roleArn`; it never falls back
silently to `GetSessionToken`. Workload identity also requires a role ARN.
Use `stsEndpoint` to override the STS address. IAM identities, buckets and
network access for external workers must be provided separately.

| Backend | What the API expresses | What must be verified |
| --- | --- | --- |
| AWS S3 | EKS workload identity with `assumeRole`; static keys with a selected STS mode. | Planned: IAM trust and permissions, client-token flow, snapshot/restore, and failover. |
| RustFS | Secret, custom endpoint, path-style, AssumeRole. | Exercised with RustFS 1.0.0: S3 writes, KV snapshots, disk replacement and client-issued tokens. See the limits below. |

With AWS or custom endpoints, omitting delegation blocks new workloads.
The CRD requires a role for workload identity and rejects `getSessionToken`
combined with a role. Apply the matching CRD when upgrading. The operator
checks Secret or ServiceAccount references before starting new workloads;
`RemoteStorageReady` does **not** check S3, STS or external reachability.
RustFS flows have been exercised with IAM user keys; AWS remains untested.

## Recovery from remote snapshots

Fluss can rebuild a lost TabletServer disk from surviving replicas and remote
KV snapshots without operator orchestration. This was exercised with RustFS,
including follower replacement and promotion. The operator reports health;
it does not move data. With RF=1, a successful snapshot recovery does **not**
prove that every acknowledged write survived: check `DataAtRisk`. Do not treat
remote snapshots as a guarantee of zero data loss after losing all live replicas.

## Client-token flow via AssumeRole

With `AssumeRole`, Fluss obtains session credentials from the backend STS.
Clients obtain filesystem tokens through `GetFileSystemSecurityToken`.
Against RustFS, a token issued from an IAM user with bucket access could
read a KV snapshot object. This does not establish the AWS IAM or external
worker network path. RustFS accepts the preset's conventional role ARN for
compatibility; the signing IAM user's policy determines permissions.

Sources: [Fluss 1.0 S3 configuration](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/), [Fluss 1.0 secret providers](https://fluss.apache.org/docs/security/secrets/), [EKS IRSA](https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html), [EKS Pod Identity](https://docs.aws.amazon.com/eks/latest/userguide/pod-identities.html), and [RustFS STS documentation](https://docs.rustfs.com/en/security-compliance/iam/sts).
