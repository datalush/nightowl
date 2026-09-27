// SPDX-License-Identifier: AGPL-3.0-only
//! Dynamic config application (j5v3 slice): push allowlisted keys live via
//! `Admin.alterClusterConfigs` instead of rolling pods.
//!
//! The step is pure planning plus one guarded RPC round-trip:
//! - Desired appliable keys come from the merged per-role properties
//!   (cluster-wide only: divergent role values stay restart-bound).
//! - The standing applied hashes in `.status` make everything idempotent
//!   across restarts and replays; only real diffs dial Fluss.
//! - Gate: a fresh (this-pass) health observation strictly better than RED.
//!   Anything else — no probe this pass, unreachable, RED — skips silently;
//!   the 60s heartbeat retries without churning status.
//! - A server rejection surfaces `DynamicConfigBlocked` naming the key and
//!   never rolls: fail closed, retry next pass. No auto-fallback to restart
//!   in this slice (that needs upgrade sequencing, a later j5v3 piece).

use std::collections::BTreeMap;
use std::time::Duration;

use fluss::client::FlussConnection;
use fluss::config::Config as FlussConfig;
use fluss::metadata::{AlterConfig, AlterConfigOpType};

use super::Observation;
use crate::api::{ClusterHealthState, FlussCluster};
use crate::controller::fluss::bootstrap_address;
use crate::resources::{config_map, server_config::dynamic};

const APPLY_TIMEOUT: Duration = Duration::from_secs(10);

/// Plan dynamic Admin operations, applying them when due.
///
/// Returns observations for the status writer; empty means nothing to do
/// or nothing safe to do right now — both read as silence, not churn.
pub async fn reconcile(cluster: &FlussCluster, observations: &[Observation]) -> Vec<Observation> {
    let desired = match desired_appliable(cluster) {
        Ok(desired) => desired,
        Err(message) => return vec![Observation::DynamicConfigBlocked { message }],
    };
    let standing = cluster
        .status
        .as_ref()
        .map(|status| status.applied_dynamic_config.clone())
        .unwrap_or_default();
    let (set, delete) = dynamic::plan(&desired, &standing);
    if set.is_empty() && delete.is_empty() {
        return Vec::new();
    }
    if !health_gate(observations) {
        return Vec::new();
    }
    let bootstrap = match bootstrap_address(cluster) {
        Some(bootstrap) => bootstrap,
        None => return Vec::new(),
    };
    match apply(&bootstrap, &set, &delete).await {
        Ok(()) => {
            tracing::info!(
                keys = ?set.iter().map(|(key, _)| key).collect::<Vec<_>>(),
                deletes = delete.len(),
                "applied dynamic config via Admin"
            );
            vec![Observation::DynamicConfigApplied {
                applied: dynamic::applied_after(&standing, &set, &delete),
            }]
        }
        Err(message) => vec![Observation::DynamicConfigBlocked { message }],
    }
}

/// Cluster-wide appliable dynamic subset, or a render-blocking message.
///
/// Render errors (forbidden keys and friends) surface as blocked: failing
/// closed here matches the config-map path, which refuses the same way.
fn desired_appliable(cluster: &FlussCluster) -> Result<BTreeMap<String, String>, String> {
    let coordinator = config_map::coordinator_properties(cluster)
        .map_err(|error| format!("dynamic config render refused: {error}"))?;
    let tablets = config_map::tablet_properties(cluster)
        .map_err(|error| format!("dynamic config render refused: {error}"))?;
    Ok(dynamic::appliable(&coordinator, &tablets))
}

/// Fresh-pass health gate: reachable with health strictly better than RED.
/// Yellow recoveries may still accept benign dynamic keys; a missing leader
/// (RED) or no fresh probe means hands off.
fn health_gate(observations: &[Observation]) -> bool {
    observations.iter().any(|observation| match observation {
        Observation::FlussHealth { health, .. } => {
            !matches!(health.status, ClusterHealthState::Red)
        }
        _ => false,
    })
}

/// One Admin round-trip: sets then deletes, with an outer timeout. Any
/// failure aborts the whole batch (nothing is recorded as applied), so a
/// later pass retries from the standing map.
async fn apply(bootstrap: &str, set: &[(String, String)], delete: &[String]) -> Result<(), String> {
    let run = async {
        let config = FlussConfig {
            bootstrap_servers: bootstrap.to_string(),
            ..Default::default()
        };
        let connection = FlussConnection::new(config)
            .await
            .map_err(|error| format!("dynamic config dial failed: {error}"))?;
        let admin = connection
            .get_admin()
            .map_err(|error| format!("dynamic config admin failed: {error}"))?;
        let mut configs: Vec<AlterConfig> = set
            .iter()
            .map(|(key, value)| {
                AlterConfig::new(key.clone(), Some(value.clone()), AlterConfigOpType::Set)
            })
            .collect();
        configs.extend(
            delete
                .iter()
                .map(|key| AlterConfig::new(key.clone(), None, AlterConfigOpType::Delete)),
        );
        admin
            .alter_cluster_configs(configs)
            .await
            .map_err(|error| format!("dynamic config alter rejected: {error}"))?;
        connection.close(Duration::from_secs(1)).await.ok();
        Ok(())
    };
    tokio::time::timeout(APPLY_TIMEOUT, run)
        .await
        .map_err(|_| "dynamic config apply timed out".to_string())?
}
