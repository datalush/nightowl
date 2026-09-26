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
use fluss::metadata::{ClusterHealth as FlussHealthData, ClusterHealthStatus as FlussHealthState};
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
/// of whoever owns cluster health.
fn bootstrap_address(cluster: &FlussCluster) -> Option<String> {
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
    connection.close(Duration::from_secs(1)).await.ok();
    Ok(snapshot(&health, &servers))
}

fn snapshot_observations(snapshot: &HealthSnapshot) -> Observation {
    Observation::FlussHealth {
        health: snapshot.health.clone(),
        coordinator_endpoints: snapshot.coordinator_endpoints.clone(),
        coordinator_ready: snapshot.coordinator_ready,
        tablet_uids: snapshot.tablet_uids.clone(),
    }
}

/// Map wire types to our status types. Membership is Fluss-observed (who
/// the coordinator sees), which is strictly more honest than pod Ready for
/// "how many servers serve". Leader identity and per-tablet assignment stay
/// absent: the membership API does not know them (erbh territory).
fn snapshot(health: &FlussHealthData, servers: &[ServerNode]) -> HealthSnapshot {
    let coordinators: Vec<String> = servers
        .iter()
        .filter(|node| matches!(node.server_type(), ServerType::CoordinatorServer))
        .map(|node| node.url())
        .collect();
    let tablet_uids: Vec<String> = servers
        .iter()
        .filter(|node| matches!(node.server_type(), ServerType::TabletServer))
        .map(|node| node.uid().to_string())
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
    }
}

/// Tablet status entries from observed membership: one per registered
/// server, named by its Fluss uid. Assignment and replica health stay
/// absent until the per-server read API exists.
pub fn tablet_entries(uids: &[String]) -> Vec<TabletServerPodStatus> {
    uids.iter()
        .map(|uid| TabletServerPodStatus {
            name: uid.clone(),
            ready: true,
            assigned_tablets: None,
            replica_health: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::snapshot;
    use crate::api::ClusterHealthState;
    use fluss::metadata::{
        ClusterHealth as FlussHealthData, ClusterHealthStatus as FlussHealthState,
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

    #[test]
    fn green_maps_with_membership() {
        let servers = vec![
            node(0, ServerType::CoordinatorServer),
            node(0, ServerType::TabletServer),
            node(1, ServerType::TabletServer),
        ];
        let snapshot = snapshot(&health(FlussHealthState::Green), &servers);
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
    }

    #[test]
    fn degraded_states_pass_through() {
        for (input, _expected) in [
            (FlussHealthState::Yellow, ClusterHealthState::Yellow),
            (FlussHealthState::Red, ClusterHealthState::Red),
            (FlussHealthState::Unknown, ClusterHealthState::Unknown),
        ] {
            let snapshot = snapshot(&health(input), &[]);
            assert_eq!(
                std::mem::discriminant(&snapshot.health.status),
                std::mem::discriminant(&_expected)
            );
        }
    }
}
