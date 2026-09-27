// SPDX-License-Identifier: AGPL-3.0-only
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
use crate::api::{
    ConditionStatus, FlussCluster, FlussClusterStatus, FlussConditionType, GatewayStatus,
};
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
    // Dynamic config: a rejection is a stall the status must explain (j5v3);
    // applied hashes replace the field wholesale when this pass applied.
    if let Some(message) = observations.iter().find_map(|o| match o {
        Observation::DynamicConfigBlocked { message } => Some(message),
        _ => None,
    }) {
        conditions.push(common::condition(
            cluster,
            FlussConditionType::OperationBlocked,
            ConditionStatus::True,
            "DynamicConfigRejected".to_string(),
            message.clone(),
            Vec::new(),
        ));
    }

    FlussClusterStatus {
        observed_generation: cluster.metadata.generation,
        observed_version: Some(cluster.spec.version.clone()),
        observed_config_hash: observations.iter().find_map(|o| match o {
            Observation::ConfigHash { value } => Some(value.clone()),
            _ => None,
        }),
        gateway: gateway_status(cluster, observations),
        applied_dynamic_config: observations
            .iter()
            .find_map(|o| match o {
                Observation::DynamicConfigApplied { applied } => Some(applied.clone()),
                _ => None,
            })
            .or_else(|| {
                cluster
                    .status
                    .as_ref()
                    .map(|status| status.applied_dynamic_config.clone())
            })
            .unwrap_or_default(),
        cluster_health,
        coordinator_endpoints,
        coordinator,
        tablet_servers,
        conditions,
    }
}

/// Gateway presence: fresh deployment observation wins, explicit absence
/// clears, otherwise the standing value survives (steady state is
/// write-free). Desired replicas mirror the spec default of 1.
fn gateway_status(cluster: &FlussCluster, observations: &[Observation]) -> Option<GatewayStatus> {
    if observations
        .iter()
        .any(|o| matches!(o, Observation::GatewayAbsent))
    {
        return None;
    }
    if let Some(available) = observations.iter().find_map(|o| match o {
        Observation::GatewayConverged {
            available: Some(available),
            ..
        } => Some(*available),
        _ => None,
    }) {
        let namespace = cluster.metadata.namespace.clone().unwrap_or_default();
        let name = cluster.metadata.name.clone().unwrap_or_default();
        let desired = cluster
            .spec
            .gateway
            .as_ref()
            .map(|spec| spec.replicas)
            .unwrap_or(1);
        return Some(GatewayStatus {
            desired,
            ready: available,
            url: format!("http://{name}-gateway.{namespace}.svc.cluster.local:8080"),
        });
    }
    cluster
        .status
        .as_ref()
        .and_then(|status| status.gateway.clone())
}
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
