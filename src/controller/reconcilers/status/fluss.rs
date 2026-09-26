//! Fluss-side status: health fields plus the `FlussReachable` and
//! `ClusterHealthy` conditions, all decided on observed data only.
//!
//! Health between probes stands as last observed (at most a minute old, per
//! the probe clock) instead of flapping to absent and back. Never observed
//! at all stays absent — the honest initial state.

use super::super::Observation;
use super::common::{carried, condition};
use crate::api::{
    ClusterHealthState, ClusterHealthStatus, ConditionStatus, CoordinatorStatus, FlussCluster,
    FlussClusterCondition, FlussConditionType, TabletServersStatus,
};
use crate::controller::fluss::tablet_entries;

/// Fresh-or-standing health fields for the status body.
pub(super) fn fields(
    cluster: &FlussCluster,
    observations: &[Observation],
) -> (
    Option<ClusterHealthStatus>,
    Vec<String>,
    Option<CoordinatorStatus>,
    Option<TabletServersStatus>,
) {
    let previous = cluster.status.as_ref();

    let health = observations.iter().find_map(|o| match o {
        Observation::FlussHealth { health, .. } => Some(health.clone()),
        _ => None,
    });
    let cluster_health = health.or_else(|| previous.and_then(|s| s.cluster_health.clone()));

    let fresh_endpoints = observations.iter().find_map(|o| match o {
        Observation::FlussHealth {
            coordinator_endpoints,
            ..
        } => Some(coordinator_endpoints.clone()),
        _ => None,
    });
    let coordinator_endpoints = fresh_endpoints.unwrap_or_else(|| {
        previous
            .map(|s| s.coordinator_endpoints.clone())
            .unwrap_or_default()
    });

    let coordinator = observations
        .iter()
        .find_map(|o| match o {
            Observation::FlussHealth {
                coordinator_ready, ..
            } => Some(CoordinatorStatus {
                desired: cluster.spec.coordinator.replicas,
                ready: *coordinator_ready,
                active_pod: None,
            }),
            _ => None,
        })
        .or_else(|| previous.and_then(|s| s.coordinator.clone()));

    let tablet_servers = observations
        .iter()
        .find_map(|o| match o {
            Observation::FlussHealth { tablet_uids, .. } => Some(TabletServersStatus {
                desired: cluster.spec.tablet_servers.replicas,
                ready: tablet_uids.len() as i32,
                pods: tablet_entries(tablet_uids),
            }),
            _ => None,
        })
        .or_else(|| previous.and_then(|s| s.tablet_servers.clone()));

    (
        cluster_health,
        coordinator_endpoints,
        coordinator,
        tablet_servers,
    )
}

/// `FlussReachable` and `ClusterHealthy` from a fresh probe, else standing
/// values. An unreachable cluster reports `FlussReachable` False and leaves
/// `ClusterHealthy` alone; stale-at-most-a-minute beats flapping.
pub(super) fn conditions(
    cluster: &FlussCluster,
    observations: &[Observation],
) -> Vec<FlussClusterCondition> {
    let mut conditions = Vec::with_capacity(2);
    let fresh_health = observations.iter().find_map(|o| match o {
        Observation::FlussHealth {
            health,
            coordinator_endpoints,
            ..
        } => Some((health, coordinator_endpoints)),
        _ => None,
    });
    let fresh_unreachable = observations.iter().find_map(|o| match o {
        Observation::FlussUnreachable { message } => Some(message),
        _ => None,
    });

    match (fresh_health, fresh_unreachable) {
        (Some((health, endpoints)), _) => {
            let evidence: Vec<String> = endpoints
                .iter()
                .map(|endpoint| format!("coordinator {endpoint} reachable"))
                .collect();
            conditions.push(condition(
                cluster,
                FlussConditionType::FlussReachable,
                ConditionStatus::True,
                "FlussReachable".to_string(),
                format!("coordinator reachable at {}", endpoints.join(", ")),
                evidence,
            ));
            let counts = format!(
                "replicas {}/{} in sync, leaders {}/{} active",
                health.replicas.in_sync_replicas,
                health.replicas.num_replicas,
                health.replicas.active_leader_replicas,
                health.replicas.num_leader_replicas,
            );
            let (status, reason) = match health.status {
                ClusterHealthState::Green => (ConditionStatus::True, "ClusterHealthy"),
                ClusterHealthState::Yellow | ClusterHealthState::Red => {
                    (ConditionStatus::False, "ClusterUnhealthy")
                }
                ClusterHealthState::Unknown => (ConditionStatus::Unknown, "ClusterHealthUnknown"),
            };
            conditions.push(condition(
                cluster,
                FlussConditionType::ClusterHealthy,
                status,
                reason.to_string(),
                counts.clone(),
                vec![counts],
            ));
        }
        (None, Some(message)) => {
            conditions.push(condition(
                cluster,
                FlussConditionType::FlussReachable,
                ConditionStatus::False,
                "FlussUnreachable".to_string(),
                message.clone(),
                vec![message.clone()],
            ));
            if let Some(standing) = carried(cluster, &FlussConditionType::ClusterHealthy) {
                conditions.push(standing);
            }
        }
        (None, None) => {
            conditions.extend(carried(cluster, &FlussConditionType::FlussReachable));
            conditions.extend(carried(cluster, &FlussConditionType::ClusterHealthy));
        }
    }
    conditions
}
