# ADR-0001: Single S3 directory per FlussCluster; multi-location deferred

- **Date:** 2026-09-26
- **Status:** Accepted

## Context

Fluss 1.0 supports `remote.data.dirs` as a list of remote directories with
selection strategies (`ROUND_ROBIN`, `WEIGHTED_ROUND_ROBIN`). Our
`FlussCluster` CR models exactly one S3 location (`remoteStorage.s3` with a
single `bucket` + `prefix`). During implementation of the `server.yaml`
renderer, the question arose whether the CR is wrong or incomplete for not
accepting a list.

## Decision

Keep a single S3 directory per `FlussCluster`:

- Render it into the **plural** `remote.data.dirs` key as a one-element
  location (`s3://<bucket>/<prefix>`). Fluss 1.0 recommends `remote.data.dirs`
  for new clusters even with a single location, so this is the documented
  posture, not a workaround.
- Defer multi-location support. A real multi-location model would require
  per-location backend configuration (region, endpoint, credentials per
  location) — a section redesign, not a comma. Our flat `s3` shape
  (one region, one endpoint, one authentication) cannot express that, and
  turning `prefix` alone into a list would only cover the weakest case
  (several prefixes on the same backend), which nobody has asked for.

## Consequences

- One backend per cluster (MinIO, Garage, real S3) is fully covered; the
  existing per-cluster prefix-dedication rule is unchanged.
- Remote locations stay effectively immutable after data is written
  (changing them requires a migration, not a config edit), so a list would
  add no operational agility today.
- No premature abstraction: per the project roadmap, we do not build
  multi-backend topology without a verified use case.

## What reopens this

Evidence of a real need, e.g. backend migration between two live stores or
a multi-region deployment, plus confirmation of how Fluss behaves across
directories (selection strategy, re-balance semantics, per-location
failure). The `v1alpha1` API allows adding locations additively when that
evidence exists.
