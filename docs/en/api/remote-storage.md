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

With no provider, AWS defaults apply. **Path-style addressing is always on**;
there is no S3 addressing knob in the CR. A custom S3 endpoint does **not**
select another provider: configure delegation explicitly for other S3-compatible
services, including their STS endpoint. `AssumeRole` is the default: AWS and
custom endpoints require a real `delegation.roleArn`. `GetSessionToken` is
available only through an explicit `delegation.type: getSessionToken`. Changing
`provider` or delegation on a running cluster is restart-bound; do not change
bucket or prefix without a data migration. Existing experimental CRs without
a role must be updated or recreated before upgrading the operator.

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

The ServiceAccount's IAM role and `delegation.roleArn` are **not the same setting**. The first identifies the Fluss server, which needs S3 access and `sts:AssumeRole` permission. The second is the role Fluss assumes to issue temporary credentials to clients. A role ARN implies `AssumeRole` without writing `type`; `provider: aws` may be specified or omitted. Fluss 1.0 requires an assumed role when server credentials come from the default AWS chain. EKS Pod Identity uses the SDK container credential provider; integration with Fluss must still be exercised in EKS before claiming a tested deployment.

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

Fluss resolves these markers at startup, so rotating a Secret requires restarting the relevant servers. The mount is implemented (verified live against RustFS). Rotation is detected, not healed: pods pin the rendered secret hash in their template, and a mismatch with the live Secret surfaces `S3CredentialsStale=True` naming the affected pod ordinals and the stale mount — without rolling anything. Restart policy is separate. AWS static-key users also need a role unless they explicitly select `getSessionToken`.

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

The server's ability to read and write S3 objects does not imply that Fluss can issue credentials to Flink/Spark clients reading remote data. Fluss 1.0's own fallback calls `GetSessionToken` with static keys; the Operator **does not** silently use that fallback: it defaults to `AssumeRole` and requires a real `delegation.roleArn` for AWS/custom services. An explicit `type: getSessionToken` selects the alternative where supported. With workload identity, `delegation.roleArn` is required. For S3-compatible services, `stsEndpoint` overrides the STS address in either mode. The provider preset never creates IAM identities, roles, buckets, or an endpoint reachable from external Spark workers; those remain platform inputs.

| Backend | What the API expresses | What must be verified |
| --- | --- | --- |
| AWS S3 | EKS workload identity with `assumeRole`; static keys with a selected STS mode. | Planned: IAM trust and permissions, client-token flow, snapshot/restore, and failover. |
| RustFS | Secret, custom endpoint, path-style, AssumeRole. | Verified 2026-09-26 against the lab instance (1.0.0): bucket round-trip from cluster pods, `AssumeRole` credentials authorizing S3 operations, KV snapshots written by operator-managed tablets under the cluster prefix, and single-disk recovery from those snapshots (see below). Fluss-issued client tokens remain separate. |

With AWS or custom endpoints, omitting `delegation` blocks new workloads with a clear message instead of silently calling AWS STS. The generated CRD requires a role for workload identity and rejects `getSessionToken` combined with a role. Reapply the generated CRD before using profiles or omitting `delegation.type`; an older installed CRD may reject the new shape. The reconciler verifies the Secret or ServiceAccount references and blocks invalid preset combinations before starting new workloads; `RemoteStorageReady` still observes **references**, not S3, STS or external network reachability. Basic S3 operations, KV snapshots, single-disk recovery, and the Fluss-issued client-token flow are verified against lab RustFS with an IAM user (separate tests); AWS remains unverified. The examples are API manifests, not compatibility certifications.

## Recovery from remote snapshots

Single TabletServer disk loss with surviving replicas recovers natively: the replacement pod rejoins and catches up from its peers plus the remote KV snapshots, with no operator orchestration. Verified 2026-09-26 against lab RustFS (RF=3, 100 rows written, one PVC plus pod deleted): the fresh pod was Ready in about four minutes with no manual steps, conditions stayed truthful throughout, and all 100 rows read back afterwards. The operator role is observe-and-report via `ClusterHealthy` and the per-area conditions. Total loss with no live replica anywhere is unrestorable by any flow — there is nowhere to recover from — and stands as a documented limit, not pending work.

## Client-token flow via AssumeRole

With `delegation: { type: assumeRole, roleArn, stsEndpoint }` the servers obtain S3 session credentials via AssumeRole against the backend STS (server log shows `S3DelegationTokenProvider … Obtaining session credentials via AssumeRole`), and clients fetch filesystem security tokens through the `GetFileSystemSecurityToken` RPC. Verified 2026-09-27 against lab RustFS with a lab-scoped IAM user (`fluss-clients`, policy limited to the lab bucket): 50 rows written and read back, snapshots flowing, a client-fetched token (access key, secret, session JWT) listing and reading a real snapshot `_METADATA` object. The backend accepts the role ARN for AWS compatibility and derives the session from the signing credential's policies; no explicit `sts:AssumeRole` policy statement was required (the policy validator rejects `Resource: "*"`).

Sources: [Fluss 1.0 S3 configuration](https://fluss.apache.org/docs/maintenance/tiered-storage/filesystems/s3/), [Fluss 1.0 secret providers](https://fluss.apache.org/docs/security/secrets/), [EKS IRSA](https://docs.aws.amazon.com/eks/latest/userguide/iam-roles-for-service-accounts.html), [EKS Pod Identity](https://docs.aws.amazon.com/eks/latest/userguide/pod-identities.html), and [RustFS STS documentation](https://docs.rustfs.com/en/security-compliance/iam/sts).
