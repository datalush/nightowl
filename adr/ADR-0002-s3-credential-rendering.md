# ADR-0002: S3 credential rendering in `server.yaml`

- **Date:** 2026-09-26
- **Status:** Accepted

## Context

The generated `server.yaml` needs S3 access configuration for remote
storage, and the CR offers two authentication modes (`secret` and
`workloadIdentity`) plus optional STS delegation. Credential *values* must
never land in a ConfigMap (readable from etcd by design). Fluss 1.0 supports
a directory secrets provider (`${directory:…}` markers resolved at startup)
and `AssumeRole` delegation for issuing client tokens; the CRD already
requires `assumeRole` delegation with `workloadIdentity` via a CEL rule.

## Decision

Render credentials by reference, per branch, with no values anywhere:

- **`Secret` branch** → render the four markers only
  (`config.providers: directory`, the `allowed.paths` entry, `s3.access-key`
  and `s3.secret-key` as `${directory:/etc/fluss/secrets/…}` references).
  The Secret's name and key names select *what* to mount, never appear as
  values.
- **`WorkloadIdentity` branch** → render **no credential keys at all**
  (and no `config.providers` block). The SDK default chain authenticates;
  markers would point at files that do not exist. The empty match arm is
  deliberate, not an omission.
- **Delegation** → `AssumeRole` adds `s3.assumed.role.arn` (plus
  `s3.assumed.role.sts.endpoint` when set); `GetSessionToken` or absent
  delegation adds nothing. The ARN comes from the `delegation` section,
  never from authentication.
- **Mount-path contract:** the markers assume the Secret mounted
  read-only at `/etc/fluss/secrets`. The future StatefulSet phase must
  honor exactly that path, or the markers dangle.

## Consequences

- Rotating a Secret requires restarting the servers (Fluss resolves
  markers once at startup).
- `workloadIdentity` clusters carry zero secret material in Kubernetes
  manifests.
- The mount path is a cross-phase contract between this renderer and the
  workload phase; changing it breaks both sides silently.

## What reopens this

Fluss changing its secrets mechanism, a second credential source beyond
Secret/IRSA, or verified EKS integration evidence contradicting the
assumptions above (e.g. a backend requiring static keys alongside workload
identity).
