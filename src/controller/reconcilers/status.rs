//! Observed-only `.status` writer.
//!
//! Renders the desired `FlussClusterStatus` from the steps' [`Observation`]s
//! and writes it only when it differs. Fluss health stays absent until the
//! controller can actually observe it; an absent field is honest, an
//! invented one is not.

use chrono::SecondsFormat;
use kube::Api;
use kube::api::PostParams;

use super::Observation;
use crate::api::{
    ConditionStatus, FlussCluster, FlussClusterCondition, FlussClusterStatus, FlussConditionType,
};
use crate::controller::Error;
use crate::controller::apply::ApplyOutcome;

/// Render the desired status from observations and write it if it changed.
///
/// Returns true when a write happened. The boolean lets the coordinator log
/// the write; skipping no-op writes avoids the
/// write -> watch event -> reconcile -> write loop.
pub async fn reconcile(
    api: &Api<FlussCluster>,
    cluster: &FlussCluster,
    observations: &[Observation],
) -> Result<bool, Error> {
    let desired = desired_status(cluster, observations);
    write_if_changed(api, cluster, &desired).await
}

/// Build the desired status purely from observed state.
///
/// Carries `observedGeneration`/`observedVersion` from the handled object and
/// reports one `KubernetesResourcesReady` condition derived from the Service
/// observations. The transition timestamp is preserved when the
/// (type, status, reason) triple is unchanged, so steady-state reconciles do
/// not rewrite history.
fn desired_status(cluster: &FlussCluster, observations: &[Observation]) -> FlussClusterStatus {
    let (condition_status, reason, message, evidence) = service_condition(observations);
    let (storage_status, storage_reason, storage_message, storage_evidence) =
        storage_condition(observations);

    FlussClusterStatus {
        observed_generation: cluster.metadata.generation,
        observed_version: Some(cluster.spec.version.clone()),
        observed_config_hash: observations.iter().find_map(|o| match o {
            Observation::ConfigHash { value } => Some(value.clone()),
            _ => None,
        }),
        conditions: vec![
            FlussClusterCondition {
                condition_type: FlussConditionType::KubernetesResourcesReady,
                status: condition_status.clone(),
                reason: reason.clone(),
                message,
                evidence,
                last_transition_time: transition_time(
                    cluster,
                    &FlussConditionType::KubernetesResourcesReady,
                    &condition_status,
                    &reason,
                ),
            },
            FlussClusterCondition {
                condition_type: FlussConditionType::RemoteStorageReady,
                status: storage_status.clone(),
                reason: storage_reason.clone(),
                message: storage_message,
                evidence: storage_evidence,
                last_transition_time: transition_time(
                    cluster,
                    &FlussConditionType::RemoteStorageReady,
                    &storage_status,
                    &storage_reason,
                ),
            },
        ],
        ..Default::default()
    }
}

/// Reduce the Service and ConfigMap observations to one condition.
///
/// A block wins over convergence: if any Service or ConfigMap is blocked,
/// the condition is False regardless of the others. Service blocks take
/// precedence in the message only because they were checked first.
fn service_condition(
    observations: &[Observation],
) -> (ConditionStatus, String, String, Vec<String>) {
    if let Some((name, message)) = observations.iter().find_map(|o| match o {
        Observation::ServiceBlocked { name, message } => Some((name.clone(), message.clone())),
        _ => None,
    }) {
        return (
            ConditionStatus::False,
            "ServiceBlocked".to_string(),
            message,
            vec![format!("service {name} exists with a different owner")],
        );
    }

    if let Some((name, message)) = observations.iter().find_map(|o| match o {
        Observation::ConfigMapBlocked { name, message } => Some((name.clone(), message.clone())),
        _ => None,
    }) {
        return (
            ConditionStatus::False,
            "ConfigBlocked".to_string(),
            message,
            vec![format!("configmap {name} blocked")],
        );
    }

    let (reason, detail, name) = observations
        .iter()
        .find_map(|o| match o {
            Observation::ServiceConverged { name, outcome } => {
                let (reason, detail) = match outcome {
                    ApplyOutcome::Created => ("ServiceCreated", "created"),
                    ApplyOutcome::Updated => ("ServiceUpdated", "updated"),
                    ApplyOutcome::Unchanged => ("ServiceConverged", "converged"),
                };
                Some((reason.to_string(), detail.to_string(), name.clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            (
                "ServiceConverged".to_string(),
                "converged".to_string(),
                "unknown".to_string(),
            )
        });

    let mut evidence = vec![format!("service {name} {detail}")];
    for o in observations {
        if let Observation::ConfigMapConverged { name, outcome } = o {
            let detail = match outcome {
                ApplyOutcome::Created => "created",
                ApplyOutcome::Updated => "updated",
                ApplyOutcome::Unchanged => "converged",
            };
            evidence.push(format!("configmap {name} {detail}"));
        }
        if let Observation::ConfigHash { value } = o {
            evidence.push(format!("config hash {value}"));
        }
    }

    (
        ConditionStatus::True,
        reason,
        format!("coordinator service {name} {detail}"),
        evidence,
    )
}

/// Reduce the storage guardrail observations to one condition.
///
/// The preflight always reports exactly one observation, so this condition
/// always resolves True or False — never unknown.
fn storage_condition(
    observations: &[Observation],
) -> (ConditionStatus, String, String, Vec<String>) {
    if let Some((name, message)) = observations.iter().find_map(|o| match o {
        Observation::StorageBlocked { name, message } => Some((name.clone(), message.clone())),
        _ => None,
    }) {
        return (
            ConditionStatus::False,
            "StorageBlocked".to_string(),
            message,
            vec![format!("remote storage dependency '{name}' unresolved")],
        );
    }

    let mut evidence = Vec::new();
    for o in observations {
        if let Observation::StorageReady { evidence: e } = o {
            evidence.extend(e.clone());
        }
    }
    (
        ConditionStatus::True,
        "StorageReady".to_string(),
        "remote storage references resolve".to_string(),
        evidence,
    )
}

/// Keep the previous transition timestamp unless this is a real transition.
///
/// A transition is a change of the (type, status, reason) triple; steady
/// state keeps history stable across reconciles.
fn transition_time(
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

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Write the status subresource only when it differs from the current one.
///
/// Uses `replace_status` (PUT): a JSON merge patch is applied at the
/// document root, so patching `/status` would require wrapping the content
/// as `{"status": ...}` — an unwrapped status body is (correctly) pruned as
/// unknown top-level fields. PUT takes the full object and needs no wrapper.
async fn write_if_changed(
    api: &Api<FlussCluster>,
    cluster: &FlussCluster,
    desired: &FlussClusterStatus,
) -> Result<bool, Error> {
    if cluster.status.as_ref() == Some(desired) {
        return Ok(false);
    }
    let name = cluster.metadata.name.clone().ok_or(Error::MissingName)?;
    let mut object = cluster.clone();
    object.status = Some(desired.clone());
    api.replace_status(&name, &PostParams::default(), &object)
        .await
        .map_err(Error::Kube)?;
    Ok(true)
}
