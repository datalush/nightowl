//! Observed-only `.status` writer.
//!
//! Renders the desired `FlussClusterStatus` from the steps' [`Observation`]s
//! and writes it only when it differs. Fluss health stays absent until the
//! controller can actually observe it; an absent field is honest, an
//! invented one is not.
//!
//! Topic logic lives in one file per concern under [`status`](self):
//! `resources` (workload convergence), `storage` (reference preflight),
//! `fluss` (observed health); `common` holds the shared builders. This file
//! only orchestrates: assemble the pieces, write when changed.

mod common;
mod fluss;
mod resources;
mod storage;

use kube::Api;
use kube::api::PostParams;

use super::Observation;
use crate::api::{FlussCluster, FlussClusterStatus, FlussConditionType};
use crate::controller::Error;

/// Render the desired status from observations and write it if it changed.
///
/// Returns true when a write happened. The boolean lets the coordinator log
/// the write; skipping no-op writes avoids the
/// write -> watch event -> reconcile -> write loop.
///
/// A write conflict (409) retries once against a fresh read: two rapid
/// reconciles can race on the same base object, and losing that race is
/// transient, not a reconcile failure.
pub async fn reconcile(
    api: &Api<FlussCluster>,
    cluster: &FlussCluster,
    observations: &[Observation],
) -> Result<bool, Error> {
    let desired = desired_status(cluster, observations);
    write_if_changed(api, cluster, &desired, observations).await
}

/// Build the desired status purely from observed state.
///
/// Carries `observedGeneration`/`observedVersion` from the handled object,
/// one condition per topic, and the health fields. The transition timestamp
/// is preserved when the (type, status, reason) triple is unchanged, so
/// steady-state reconciles do not rewrite history.
fn desired_status(cluster: &FlussCluster, observations: &[Observation]) -> FlussClusterStatus {
    let (condition_status, reason, message, evidence) = resources::condition_tuple(observations);
    let (storage_status, storage_reason, storage_message, storage_evidence) =
        storage::condition_tuple(observations);
    let (cluster_health, coordinator_endpoints, coordinator, tablet_servers) =
        fluss::fields(cluster, observations);

    let mut conditions = vec![
        common::condition(
            cluster,
            FlussConditionType::KubernetesResourcesReady,
            condition_status,
            reason,
            message,
            evidence,
        ),
        common::condition(
            cluster,
            FlussConditionType::RemoteStorageReady,
            storage_status,
            storage_reason,
            storage_message,
            storage_evidence,
        ),
    ];
    conditions.extend(fluss::conditions(cluster, observations));
    conditions.extend(storage::secret_condition(cluster, observations));

    FlussClusterStatus {
        observed_generation: cluster.metadata.generation,
        observed_version: Some(cluster.spec.version.clone()),
        observed_config_hash: observations.iter().find_map(|o| match o {
            Observation::ConfigHash { value } => Some(value.clone()),
            _ => None,
        }),
        cluster_health,
        coordinator_endpoints,
        coordinator,
        tablet_servers,
        conditions,
    }
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
    observations: &[Observation],
) -> Result<bool, Error> {
    if cluster.status.as_ref() == Some(desired) {
        return Ok(false);
    }
    let name = cluster.metadata.name.clone().ok_or(Error::MissingName)?;
    let mut object = cluster.clone();
    object.status = Some(desired.clone());
    match api
        .replace_status(&name, &PostParams::default(), &object)
        .await
    {
        Ok(_) => Ok(true),
        // Lost a race with another writer (usually ourselves a reconcile
        // earlier): recompute against the fresh object once instead of
        // failing the whole reconcile.
        Err(kube::Error::Api(status)) if status.code == 409 => {
            let fresh: FlussCluster = api.get(&name).await.map_err(Error::Kube)?;
            let desired = desired_status(&fresh, observations);
            if fresh.status.as_ref() == Some(&desired) {
                return Ok(false);
            }
            let mut object = fresh;
            object.status = Some(desired);
            api.replace_status(&name, &PostParams::default(), &object)
                .await
                .map_err(Error::Kube)?;
            Ok(true)
        }
        Err(e) => Err(Error::Kube(e)),
    }
}
