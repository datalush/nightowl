//! The `RemoteStorageReady` condition: S3 reference preflight.
//!
//! The preflight always reports exactly one observation, so this condition
//! always resolves True or False — never unknown.

use super::super::Observation;
use crate::api::ConditionStatus;

/// Reduce the storage guardrail observations to one condition triple plus
/// evidence.
pub(super) fn condition_tuple(
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
