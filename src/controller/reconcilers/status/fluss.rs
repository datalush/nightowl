// SPDX-License-Identifier: AGPL-3.0-only
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
            Observation::FlussHealth {
                tablet_uids,
                tablet_health,
                ..
            } => Some(TabletServersStatus {
                desired: cluster.spec.tablet_servers.replicas,
                ready: tablet_uids.len() as i32,
                pods: tablet_entries(tablet_uids, tablet_health),
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
            let no_live_replica = matches!(health.status, ClusterHealthState::Red)
                && health.replicas.num_replicas > 0
                && health.replicas.active_leader_replicas == 0;
            if health.data_at_risk == Some(true) {
                let message = "Fluss recorded recovery after local log loss; writes beyond the recovered snapshot or remote-log offset cannot be verified".to_string();
                conditions.push(condition(
                    cluster,
                    FlussConditionType::DataAtRisk,
                    ConditionStatus::True,
                    "SnapshotRecoveryUnverified".to_string(),
                    message.clone(),
                    vec!["GetClusterHealth.data_at_risk=true".to_string(), message],
                ));
            } else if no_live_replica {
                let message = format!(
                    "no active leaders for {} hosted replicas ({} in sync): data unreadable until a replica recovers",
                    health.replicas.num_replicas, health.replicas.in_sync_replicas,
                );
                conditions.push(condition(
                    cluster,
                    FlussConditionType::DataAtRisk,
                    ConditionStatus::True,
                    "NoLiveReplica".to_string(),
                    message.clone(),
                    vec![message],
                ));
            } else if health.data_at_risk == Some(false) {
                conditions.push(condition(
                    cluster,
                    FlussConditionType::DataAtRisk,
                    ConditionStatus::False,
                    "ReplicasLive".to_string(),
                    "every hosted replica set has an active leader".to_string(),
                    Vec::new(),
                ));
            } else {
                let previous_risk = carried(cluster, &FlussConditionType::DataAtRisk)
                    .filter(|standing| standing.status == ConditionStatus::True);
                conditions.push(previous_risk.unwrap_or_else(|| {
                    condition(
                        cluster,
                        FlussConditionType::DataAtRisk,
                        ConditionStatus::Unknown,
                        "RecoveryEvidenceUnavailable".to_string(),
                        "Fluss did not report persistent recovery evidence".to_string(),
                        vec!["GetClusterHealth.data_at_risk absent".to_string()],
                    )
                }));
            }
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
            if let Some(standing) = carried(cluster, &FlussConditionType::DataAtRisk) {
                conditions.push(standing);
            }
        }
        (None, None) => {
            conditions.extend(carried(cluster, &FlussConditionType::FlussReachable));
            conditions.extend(carried(cluster, &FlussConditionType::ClusterHealthy));
            conditions.extend(carried(cluster, &FlussConditionType::DataAtRisk));
        }
    }
    conditions
}

#[cfg(test)]
mod tests {
    use super::conditions;
    use crate::api::{
        ClusterHealthState, ClusterHealthStatus, ConditionStatus, FlussConditionType, ReplicaHealth,
    };
    use crate::controller::reconcilers::Observation;

    fn spike_cluster() -> crate::api::FlussCluster {
        serde_yaml::from_str(include_str!("../../../../../lab2/demo.yml"))
            .expect("lab2 demo CR must deserialize")
    }

    fn health(status: ClusterHealthState, num_replicas: i32, active_leaders: i32) -> Observation {
        Observation::FlussHealth {
            health: ClusterHealthStatus {
                status,
                replicas: ReplicaHealth {
                    num_replicas,
                    in_sync_replicas: num_replicas,
                    num_leader_replicas: active_leaders,
                    active_leader_replicas: active_leaders,
                },
                data_at_risk: Some(false),
            },
            coordinator_endpoints: vec!["coord:9123".to_string()],
            coordinator_ready: 1,
            tablet_uids: vec!["ts-0".to_string()],
            tablet_health: Vec::new(),
        }
    }

    fn data_at_risk(
        cluster: &crate::api::FlussCluster,
        observations: &[Observation],
    ) -> Option<crate::api::FlussClusterCondition> {
        conditions(cluster, observations)
            .into_iter()
            .find(|c| c.condition_type == FlussConditionType::DataAtRisk)
    }

    #[test]
    fn red_without_active_leaders_reports_data_at_risk() {
        let cluster = spike_cluster();
        let observations = vec![health(ClusterHealthState::Red, 4, 0)];
        let condition = data_at_risk(&cluster, &observations)
            .expect("DataAtRisk always reported on a fresh probe");
        assert_eq!(condition.status, ConditionStatus::True);
        assert_eq!(condition.reason, "NoLiveReplica");
        assert!(
            condition.message.contains('4'),
            "evidence names the replica count, got: {}",
            condition.message
        );
    }

    #[test]
    fn green_or_active_leaders_read_clear() {
        let cluster = spike_cluster();
        for observations in [
            vec![health(ClusterHealthState::Green, 4, 2)],
            vec![health(ClusterHealthState::Red, 4, 2)],
            vec![health(ClusterHealthState::Yellow, 0, 0)],
        ] {
            let condition = data_at_risk(&cluster, &observations)
                .expect("DataAtRisk always reported on a fresh probe");
            assert_eq!(condition.status, ConditionStatus::False);
        }
    }

    #[test]
    fn green_with_persisted_recovery_still_reports_risk() {
        let cluster = spike_cluster();
        let mut observation = health(ClusterHealthState::Green, 1, 1);
        if let Observation::FlussHealth { health, .. } = &mut observation {
            health.data_at_risk = Some(true);
        }
        let condition = data_at_risk(&cluster, &[observation]).expect("risk reported");
        assert_eq!(condition.status, ConditionStatus::True);
        assert_eq!(condition.reason, "SnapshotRecoveryUnverified");
    }

    #[test]
    fn old_server_without_recovery_evidence_is_unknown() {
        let cluster = spike_cluster();
        let mut observation = health(ClusterHealthState::Green, 1, 1);
        if let Observation::FlussHealth { health, .. } = &mut observation {
            health.data_at_risk = None;
        }
        let condition = data_at_risk(&cluster, &[observation]).expect("risk reported");
        assert_eq!(condition.status, ConditionStatus::Unknown);
        assert_eq!(condition.reason, "RecoveryEvidenceUnavailable");
    }

    #[test]
    fn no_phantom_condition_without_observations() {
        let cluster = spike_cluster();
        assert!(
            data_at_risk(&cluster, &[]).is_none(),
            "nothing standing on a fresh cluster, nothing carried"
        );
    }
}
