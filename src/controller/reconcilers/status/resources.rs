// SPDX-License-Identifier: AGPL-3.0-only
//! The `KubernetesResourcesReady` condition: every managed workload kind.
//!
//! A block wins over convergence: if any Service, ConfigMap, StatefulSet,
//! budget or resource check is blocked, the condition is False regardless of
//! the others. Service blocks take precedence in the message only because
//! they were checked first.

use super::super::Observation;
use super::common::outcome_detail;
use crate::api::ConditionStatus;
use crate::controller::apply::ApplyOutcome;

/// Reduce all workload observations to one condition triple plus evidence.
pub(super) fn condition_tuple(
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

    if let Some((name, message)) = observations.iter().find_map(|o| match o {
        Observation::StatefulSetBlocked { name, message }
        | Observation::StatefulSetAwaitingExternal { name, message } => {
            Some((name.clone(), message.clone()))
        }
        _ => None,
    }) {
        return (
            ConditionStatus::False,
            "StatefulSetBlocked".to_string(),
            message,
            vec![format!("statefulset {name} blocked")],
        );
    }

    if let Some((name, message)) = observations.iter().find_map(|o| match o {
        Observation::PdbBlocked { name, message } => Some((name.clone(), message.clone())),
        _ => None,
    }) {
        return (
            ConditionStatus::False,
            "PdbBlocked".to_string(),
            message,
            vec![format!("poddisruptionbudget {name} blocked")],
        );
    }

    if let Some((name, message)) = observations.iter().find_map(|o| match o {
        Observation::GatewayBlocked { name, message } => Some((name.clone(), message.clone())),
        _ => None,
    }) {
        return (
            ConditionStatus::False,
            "GatewayBlocked".to_string(),
            message,
            vec![format!("gateway {name} blocked")],
        );
    }

    if let Some((name, message)) = observations.iter().find_map(|o| match o {
        Observation::ResourceBlocked { name, message } => Some((name.clone(), message.clone())),
        _ => None,
    }) {
        return (
            ConditionStatus::False,
            "ResourceBlocked".to_string(),
            message,
            vec![format!("{name} resources blocked")],
        );
    }

    if let Some((name, message)) = observations.iter().find_map(|o| match o {
        Observation::VolumeBlocked { name, message } => Some((name.clone(), message.clone())),
        _ => None,
    }) {
        return (
            ConditionStatus::False,
            "VolumeBlocked".to_string(),
            message,
            vec![format!("volume lifecycle for {name} blocked")],
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
                    ApplyOutcome::Deleted => ("ServiceDeleted", "deleted"),
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

    let mut evidence = Vec::new();
    for o in observations {
        if let Observation::ServiceConverged { name, outcome } = o {
            evidence.push(format!("service {name} {}", outcome_detail(outcome)));
        }
        if let Observation::GatewayConverged { name, outcome, .. } = o {
            evidence.push(format!("gateway {name} {}", outcome_detail(outcome)));
        }
    }
    for o in observations {
        if let Observation::ConfigMapConverged { name, outcome } = o {
            evidence.push(format!("configmap {name} {}", outcome_detail(outcome)));
        }
        if let Observation::ConfigHash { value } = o {
            evidence.push(format!("config hash {value}"));
        }
        if let Observation::StatefulSetConverged { name, outcome } = o {
            evidence.push(format!("statefulset {name} {}", outcome_detail(outcome)));
        }
        // Waiting is reported, never an error: the coordinator's own status
        // flips retrigger this controller, so the wait always ends.
        if let Observation::WaitingForCoordinator { name } = o {
            evidence.push(format!("statefulset {name} waiting for coordinator"));
        }
        if let Observation::PdbConverged { name, outcome } = o {
            evidence.push(format!(
                "poddisruptionbudget {name} {}",
                outcome_detail(outcome)
            ));
        }
        if let Observation::PvcResized { names } = o {
            evidence.push(format!(
                "persistentvolumeclaims resized: {}",
                names.join(", ")
            ));
        }
    }

    (
        ConditionStatus::True,
        reason,
        format!("service {name} {detail}"),
        evidence,
    )
}
