// SPDX-License-Identifier: AGPL-3.0-only
//! The `RemoteStorageReady` condition: S3 reference preflight.
//!
//! The preflight always reports exactly one observation, so this condition
//! always resolves True or False — never unknown.

use super::super::Observation;
use super::common::{carried, condition};
use crate::api::{ConditionStatus, FlussCluster, FlussClusterCondition, FlussConditionType};

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

/// The `S3CredentialsStale` condition: rotation detection only.
///
/// Fresh (or no secret auth at all) resolves False/`CredentialsInSync`; a
/// mismatch resolves True/`StaleCredentials` with the affected pod ordinals
/// and the stale mount as evidence. Uncheckable rounds carry the previous
/// value instead of flapping. Never blocks, never rolls: restart policy
/// belongs to j5v3.
pub(super) fn secret_condition(
    cluster: &FlussCluster,
    observations: &[Observation],
) -> Option<FlussClusterCondition> {
    use crate::constants::S3_SECRETS_DIR;

    let stale = observations.iter().find_map(|o| match o {
        Observation::SecretStale { pods, message } => Some((pods, message)),
        _ => None,
    });
    let fresh = observations.iter().find_map(|o| match o {
        Observation::SecretFresh { message } => Some(message),
        _ => None,
    });
    match (stale, fresh) {
        (Some((pods, message)), _) => Some(condition(
            cluster,
            FlussConditionType::S3CredentialsStale,
            ConditionStatus::True,
            "StaleCredentials".to_string(),
            message.clone(),
            pods.iter()
                .map(|pod| format!("pod {pod} mounts stale credentials from {S3_SECRETS_DIR}"))
                .collect(),
        )),
        (None, Some(message)) => Some(condition(
            cluster,
            FlussConditionType::S3CredentialsStale,
            ConditionStatus::False,
            "CredentialsInSync".to_string(),
            message.clone(),
            vec![],
        )),
        (None, None) => carried(cluster, &FlussConditionType::S3CredentialsStale),
    }
}
