# ADR-0003: Operator-owned key policy and quorum from effective RF

- **Date:** 2026-09-26
- **Status:** Accepted

## Context

`configuration_overrides` lets users tune any Fluss `server.yaml` key, but
some keys are rendered or validated by the Operator itself (identity,
topology, credentials, storage wiring, table defaults). Accepting an
override of those silently would desynchronize what the Operator guarantees
from what Fluss runs. Separately, the `min_in_sync_replicas` default
(`floor(RF/2)+1`) must track the replication factor users actually get,
including RF set through overrides rather than the structured field.

## Decision

- **Three key classes.** *Hard* keys are rejected (`identity`, `listeners`,
  ZooKeeper location, S3 wiring/credentials, `data.dir`, rendered table
  defaults): overriding them would break the cluster or silently desync
  validated wiring. *Soft* keys (today: the table defaults) and *unknown*
  future keys pass through, so new Fluss knobs keep working without code
  changes.
- **Tags live next to renderers.** Each `server_config` module declares
  its hard keys in a `PROTECTED_KEYS` constant; `overrides.rs` unions them
  with a tiny reserved-absent list (`bind/advertised.listeners`,
  `tablet-server.id`, composed per Pod and never in the shared map) and
  checks before merging — fail closed, never partially applied. No tags in
  the CRD: ownership is operator-internal machinery, and CEL would duplicate
  the list in a second language.
- **Quorum follows the effective RF.** After overrides merge, absent min-ISR
  renders `floor(effective_RF/2)+1` where effective RF resolves merged map
  → structured section → Fluss default 1. Explicit values (from anywhere)
  must parse as positive integers with `minISR ≤ effective RF`; violations
  fail closed with the key, value and reason. Rationale recorded in
  `table_defaults.rs`.
- **Refusals are visible, never retried hot.** Both failure kinds surface as
  `ConfigBlocked` status naming the key (and value/reason for invalid
  values) and settle to watch-driven retries, mirroring the intruder path.

## Consequences

- Users can tune (`kv.*`, `netty.*`) and refine defaults per role, but
  cannot silently fight the Operator over identity, topology or credentials.
- Even RF under the quorum default trades availability for durability;
  documented in both languages, not forbidden.
- Adding a tuning knob costs zero policy work; adding identity wiring costs
  one line next to the key it protects.

## What reopens this

A Fluss release changing which keys are identity vs tuning, a verified need
for per-role RF semantics differing from cluster-wide, or evidence that
admission-time (CEL) rejection is required in addition to status-visible
refusal.
