// SPDX-License-Identifier: AGPL-3.0-only
//! Best-effort Fluss-side health probe: observe, never gate.
//!
//! Dials the coordinator over the internal listener and reads cluster
//! health plus server membership. Anything failing reports absence, never
//! an error: health observation must not block convergence, retry hot, or
//! invent data. One fresh connection per probe (bounded by timeouts), and
//! probes rate-limited in memory so `.status` never churns on their account.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use fluss::client::FlussConnection;
use fluss::config::Config as FlussConfig;
use fluss::metadata::{
    ClusterHealth as FlussHealthData, ClusterHealthStatus as FlussHealthState,
    TabletServerHealth as FlussTabletHealth,
};
use fluss::{ServerNode, ServerType};

use super::reconcilers::Observation;
use crate::api::{
    ClusterHealthState, ClusterHealthStatus, FlussCluster, ReplicaHealth, TabletServerPodStatus,
};
use crate::constants::COORDINATOR_HEADLESS_SUFFIX;

/// Minimum age of the previous probe before probing again.
const PROBE_INTERVAL: Duration = Duration::from_secs(60);

/// Total budget per probe, including connect and both RPCs.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Connect timeout inside the client config; the outer [`PROBE_TIMEOUT`]
/// still bounds the whole probe.
const CONNECT_TIMEOUT_MS: u64 = 3_000;

/// Last probe per cluster, shared across reconciles. Memory-only on
/// purpose: rate limiting here must never write `.status`, so health data
/// between probes simply stands as last observed (at most [`PROBE_INTERVAL`]
/// old — documented, not hidden).
#[derive(Debug, Default)]
pub struct ProbeClock {
    last: Mutex<HashMap<String, Instant>>,
}

impl ProbeClock {
    /// True when a fresh probe is due; records the attempt either way so
    /// persistent failures also wait out the interval instead of hot-looping.
    fn due(&self, cluster: &FlussCluster) -> bool {
        let key = format!(
            "{}/{}",
            cluster.metadata.namespace.clone().unwrap_or_default(),
            cluster.metadata.name.clone().unwrap_or_default(),
        );
        let mut last = self.last.lock().expect("probe clock is never poisoned");
        match last.get(&key) {
            Some(instant) if instant.elapsed() < PROBE_INTERVAL => false,
            _ => {
                last.insert(key, Instant::now());
                true
            }
        }
    }
}

/// Snapshot of what the cluster reported, in our own status types.
#[derive(Clone, Debug)]
pub struct HealthSnapshot {
    pub health: ClusterHealthStatus,
    pub coordinator_endpoints: Vec<String>,
    pub coordinator_ready: i32,
    pub tablet_uids: Vec<String>,
    pub tablet_health: Vec<TabletHealth>,
}

/// Per-server health slice, already joined to the membership uid: the
/// four cluster counters scoped to one TabletServer. Absent when the
/// server predates the `DescribeTabletServers` API (ApiKey 1067) —
/// cluster health still reports, per-server fields stay unknown.
#[derive(Clone, Debug)]
pub struct TabletHealth {
    pub uid: String,
    pub assigned_tablets: i32,
    pub replica: ReplicaHealth,
}

/// Probe the cluster when due; `None` means "keep standing values".
///
/// Skips silently when the cluster cannot even be addressed (no name,
/// namespace or listeners yet) — the caller treats that like any other
/// absence. Reachability failures become `FlussUnreachable` observations;
/// only a full round-trip becomes `FlussHealth`.
pub async fn probe(cluster: &FlussCluster, clock: &ProbeClock) -> Vec<Observation> {
    if !clock.due(cluster) {
        return Vec::new();
    }
    let bootstrap = match bootstrap_address(cluster) {
        Some(address) => address,
        None => return Vec::new(),
    };
    match tokio::time::timeout(PROBE_TIMEOUT, round_trip(&bootstrap)).await {
        Ok(Ok(snapshot)) => vec![snapshot_observations(&snapshot)],
        Ok(Err(e)) => vec![Observation::FlussUnreachable {
            message: format!("fluss unreachable at {bootstrap}: {e}"),
        }],
        Err(_) => vec![Observation::FlussUnreachable {
            message: format!("fluss probe at {bootstrap} exceeded {PROBE_TIMEOUT:?}"),
        }],
    }
}

/// Coordinator ordinal zero over the internal listener: the stable address
/// of whoever owns cluster health. Shared with the dynamic-config step,
/// which speaks to the same Admin endpoint.
pub(crate) fn bootstrap_address(cluster: &FlussCluster) -> Option<String> {
    let name = cluster.metadata.name.clone()?;
    let namespace = cluster.metadata.namespace.clone()?;
    let port = cluster.spec.listeners.as_ref()?.internal.port;
    Some(format!(
        "{name}-coordinator-0.{name}{COORDINATOR_HEADLESS_SUFFIX}.{namespace}.svc.cluster.local:{port}",
    ))
}

async fn round_trip(bootstrap: &str) -> Result<HealthSnapshot, fluss::error::Error> {
    let config = FlussConfig {
        bootstrap_servers: bootstrap.to_string(),
        connect_timeout_ms: CONNECT_TIMEOUT_MS,
        ..Default::default()
    };
    let connection = FlussConnection::new(config).await?;
    let admin = connection.get_admin()?;
    let health = admin.get_cluster_health().await?;
    let servers = admin.get_server_nodes().await?;
    // Best-effort: stock servers answer UnsupportedVersion, and then
    // cluster health still reports while per-server fields stay unknown.
    let per_server = admin
        .describe_tablet_servers(vec![])
        .await
        .unwrap_or_default();
    connection.close(Duration::from_secs(1)).await.ok();
    Ok(snapshot(&health, &servers, &per_server))
}

fn snapshot_observations(snapshot: &HealthSnapshot) -> Observation {
    Observation::FlussHealth {
        health: snapshot.health.clone(),
        coordinator_endpoints: snapshot.coordinator_endpoints.clone(),
        coordinator_ready: snapshot.coordinator_ready,
        tablet_uids: snapshot.tablet_uids.clone(),
        tablet_health: snapshot.tablet_health.clone(),
    }
}

/// Map wire types to our status types. Membership is Fluss-observed (who
/// the coordinator sees), which is strictly more honest than pod Ready for
/// "how many servers serve". Leader identity stays absent: the membership
/// API does not know it. Per-server replica counts ride along when the
/// server answers `DescribeTabletServers`, joined by server id.
fn snapshot(
    health: &FlussHealthData,
    servers: &[ServerNode],
    per_server: &[FlussTabletHealth],
) -> HealthSnapshot {
    let coordinators: Vec<String> = servers
        .iter()
        .filter(|node| matches!(node.server_type(), ServerType::CoordinatorServer))
        .map(|node| node.url())
        .collect();
    let tablets: Vec<&ServerNode> = servers
        .iter()
        .filter(|node| matches!(node.server_type(), ServerType::TabletServer))
        .collect();
    let tablet_uids: Vec<String> = tablets.iter().map(|node| node.uid().to_string()).collect();
    // Join by server id: `ts-{id}` is the client's own uid construction,
    // so only membership-known servers ever populate. Describe entries
    // for unknown ids (stale or foreign) are ignored, never invented.
    let tablet_health: Vec<TabletHealth> = per_server
        .iter()
        .filter_map(|entry| {
            tablets
                .iter()
                .find(|node| node.id() == entry.server_id)
                .map(|node| TabletHealth {
                    uid: node.uid().to_string(),
                    assigned_tablets: entry.num_replicas,
                    replica: ReplicaHealth {
                        num_replicas: entry.num_replicas,
                        in_sync_replicas: entry.in_sync_replicas,
                        num_leader_replicas: entry.num_leader_replicas,
                        active_leader_replicas: entry.active_leader_replicas,
                    },
                })
        })
        .collect();
    HealthSnapshot {
        health: ClusterHealthStatus {
            status: match health.status {
                FlussHealthState::Green => ClusterHealthState::Green,
                FlussHealthState::Yellow => ClusterHealthState::Yellow,
                FlussHealthState::Red => ClusterHealthState::Red,
                FlussHealthState::Unknown => ClusterHealthState::Unknown,
            },
            replicas: ReplicaHealth {
                num_replicas: health.num_replicas,
                in_sync_replicas: health.in_sync_replicas,
                num_leader_replicas: health.num_leader_replicas,
                active_leader_replicas: health.active_leader_replicas,
            },
        },
        coordinator_endpoints: coordinators.clone(),
        coordinator_ready: coordinators.len() as i32,
        tablet_uids,
        tablet_health,
    }
}

/// Tablet status entries from observed membership: one per registered
/// server, named by its Fluss uid. Per-server counters ride along when
/// the server answered `DescribeTabletServers`; otherwise they stay
/// absent until observed.
pub fn tablet_entries(uids: &[String], health: &[TabletHealth]) -> Vec<TabletServerPodStatus> {
    uids.iter()
        .map(|uid| {
            let detail = health.iter().find(|h| &h.uid == uid);
            TabletServerPodStatus {
                name: uid.clone(),
                ready: true,
                assigned_tablets: detail.map(|h| h.assigned_tablets),
                replica_health: detail.map(|h| h.replica.clone()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{snapshot, tablet_entries};
    use crate::api::ClusterHealthState;
    use fluss::metadata::{
        ClusterHealth as FlussHealthData, ClusterHealthStatus as FlussHealthState,
        TabletServerHealth as FlussTabletHealth,
    };
    use fluss::{ServerNode, ServerType};

    fn health(status: FlussHealthState) -> FlussHealthData {
        FlussHealthData {
            num_replicas: 6,
            in_sync_replicas: 6,
            num_leader_replicas: 2,
            active_leader_replicas: 2,
            status,
        }
    }

    fn node(id: i32, server_type: ServerType) -> ServerNode {
        ServerNode::new(id, format!("host-{id}"), 9123 + id as u32, server_type)
    }

    fn per_server(
        server_id: i32,
        num_replicas: i32,
        in_sync_replicas: i32,
        num_leader_replicas: i32,
        active_leader_replicas: i32,
    ) -> FlussTabletHealth {
        FlussTabletHealth {
            server_id,
            num_replicas,
            in_sync_replicas,
            num_leader_replicas,
            active_leader_replicas,
        }
    }

    #[test]
    fn green_maps_with_membership() {
        let servers = vec![
            node(0, ServerType::CoordinatorServer),
            node(0, ServerType::TabletServer),
            node(1, ServerType::TabletServer),
        ];
        let snapshot = snapshot(&health(FlussHealthState::Green), &servers, &[]);
        assert!(matches!(snapshot.health.status, ClusterHealthState::Green));
        assert_eq!(snapshot.health.replicas.num_replicas, 6);
        assert_eq!(snapshot.coordinator_ready, 1);
        assert_eq!(
            snapshot.coordinator_endpoints,
            vec!["host-0:9123".to_string()]
        );
        assert_eq!(
            snapshot.tablet_uids,
            vec!["ts-0".to_string(), "ts-1".to_string()]
        );
        assert!(
            snapshot.tablet_health.is_empty(),
            "no describe answer means unknown per-server health, not zeros"
        );
    }

    #[test]
    fn degraded_states_pass_through() {
        for (input, _expected) in [
            (FlussHealthState::Yellow, ClusterHealthState::Yellow),
            (FlussHealthState::Red, ClusterHealthState::Red),
            (FlussHealthState::Unknown, ClusterHealthState::Unknown),
        ] {
            let snapshot = snapshot(&health(input), &[], &[]);
            assert_eq!(
                std::mem::discriminant(&snapshot.health.status),
                std::mem::discriminant(&_expected)
            );
        }
    }

    #[test]
    fn per_server_health_joins_membership_by_id() {
        let servers = vec![
            node(0, ServerType::CoordinatorServer),
            node(0, ServerType::TabletServer),
            node(2, ServerType::TabletServer),
        ];
        let describe = vec![
            per_server(0, 4, 4, 2, 2),
            per_server(2, 4, 3, 2, 1),
            per_server(9, 1, 1, 0, 0),
        ];
        let snapshot = snapshot(&health(FlussHealthState::Yellow), &servers, &describe);
        assert_eq!(snapshot.tablet_health.len(), 2);
        assert_eq!(snapshot.tablet_health[0].uid, "ts-0");
        assert_eq!(snapshot.tablet_health[0].assigned_tablets, 4);
        assert_eq!(snapshot.tablet_health[0].replica.in_sync_replicas, 4);
        assert_eq!(snapshot.tablet_health[1].uid, "ts-2");
        assert_eq!(snapshot.tablet_health[1].replica.active_leader_replicas, 1);

        let entries = tablet_entries(&snapshot.tablet_uids, &snapshot.tablet_health);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].assigned_tablets, Some(4));
        assert_eq!(
            entries[0]
                .replica_health
                .as_ref()
                .expect("joined")
                .num_replicas,
            4
        );
        assert_eq!(entries[1].assigned_tablets, Some(4));
    }

    #[test]
    fn members_missing_from_describe_stay_unknown() {
        let servers = vec![node(0, ServerType::TabletServer)];
        let snapshot = snapshot(&health(FlussHealthState::Green), &servers, &[]);
        let entries = tablet_entries(&snapshot.tablet_uids, &snapshot.tablet_health);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].assigned_tablets, None);
        assert_eq!(entries[0].replica_health, None);
    }
}
