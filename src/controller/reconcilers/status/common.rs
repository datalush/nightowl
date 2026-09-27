// SPDX-License-Identifier: AGPL-3.0-only
//! Shared status-rendering helpers: no topic logic here.
//!
//! Every topic module (`resources`, `storage`, `fluss`) builds its piece
//! through these, so conditions stay uniform: history-preserving transition
//! times, one-word outcome details, and standing-value carry-forward.

use chrono::SecondsFormat;

use crate::api::{ConditionStatus, FlussCluster, FlussClusterCondition, FlussConditionType};
use crate::controller::apply::ApplyOutcome;

/// One condition with history-preserving transition time.
pub(super) fn condition(
    cluster: &FlussCluster,
    condition_type: FlussConditionType,
    status: ConditionStatus,
    reason: String,
    message: String,
    evidence: Vec<String>,
) -> FlussClusterCondition {
    FlussClusterCondition {
        condition_type: condition_type.clone(),
        status: status.clone(),
        reason: reason.clone(),
        message,
        evidence,
        last_transition_time: transition_time(cluster, &condition_type, &status, &reason),
    }
}

/// Keep the previous transition timestamp unless this is a real transition.
///
/// A transition is a change of the (type, status, reason) triple; steady
/// state keeps history stable across reconciles.
pub(super) fn transition_time(
    cluster: &FlussCluster,
    condition_type: &FlussConditionType,
    status: &ConditionStatus,
    reason: &str,
) -> String {
    cluster
        .status
        .as_ref()
        .map(|s| s.conditions.as_slice())
        .unwrap_or(&[])
        .iter()
        .find(|c| c.condition_type == *condition_type && c.status == *status && c.reason == reason)
        .map(|c| c.last_transition_time.clone())
        .unwrap_or_else(now_rfc3339)
}

/// Previous condition of a type, when the current reconcile observed
/// nothing about it. Standing values beat flapping to absent and back.
pub(super) fn carried(
    cluster: &FlussCluster,
    condition_type: &FlussConditionType,
) -> Option<FlussClusterCondition> {
    cluster
        .status
        .as_ref()
        .map(|s| s.conditions.as_slice())
        .unwrap_or(&[])
        .iter()
        .find(|c| &c.condition_type == condition_type)
        .cloned()
}

/// One-word evidence detail for a converge outcome.
pub(super) fn outcome_detail(outcome: &ApplyOutcome) -> &'static str {
    match outcome {
        ApplyOutcome::Created => "created",
        ApplyOutcome::Updated => "updated",
        ApplyOutcome::Unchanged => "converged",
        ApplyOutcome::Deleted => "deleted",
    }
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
