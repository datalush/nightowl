// SPDX-License-Identifier: AGPL-3.0-only
//! Runtime precondition checks (guardrails).
//!
//! Unlike admission-time CEL rules, these read live cluster state or cover
//! upgrade windows where the installed CRD predates a validation rule.
//! Every check reports through observations so blocks surface in `.status`
//! with evidence; the coordinator turns them into errors *after* the status
//! write, reusing the fail-closed, no-hot-loop policy.

pub mod ownership;
pub mod replication;
pub mod resources;
pub mod storage;
