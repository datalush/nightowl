// SPDX-License-Identifier: AGPL-3.0-only
use std::sync::Arc;
use std::time::Duration;

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{ConfigMap, PersistentVolumeClaim, Secret, Service};
use k8s_openapi::api::networking::v1::Ingress;
use k8s_openapi::api::policy::v1::PodDisruptionBudget;
use k8s_openapi::api::storage::v1::StorageClass;
use kube::Api;
use kube::runtime::controller::Action;

use super::fluss;
use super::reconcilers::{self, Observation};
use super::{Context, Error};
use crate::api::FlussCluster;
use crate::controller::guardrails;

/// Coordinate one FlussCluster reconciliation.
///
/// Runs each step in order, collects their [`Observation`]s, reports them
/// in `.status`, and surfaces a block as an error afterwards so the error
/// policy still applies. This function only orchestrates: adding a resource
/// means a new step plus a few lines here, never a longer coordinator.
pub async fn reconcile(cluster: Arc<FlussCluster>, ctx: Arc<Context>) -> Result<Action, Error> {
    let name = cluster.metadata.name.clone().ok_or(Error::MissingName)?;
    let namespace = cluster
        .metadata
        .namespace
        .clone()
        .ok_or(Error::MissingNamespace)?;
    let uid = cluster.metadata.uid.clone().ok_or(Error::MissingUid)?;

    let services: Api<Service> = Api::namespaced(ctx.client.clone(), &namespace);
    let configmaps: Api<ConfigMap> = Api::namespaced(ctx.client.clone(), &namespace);
    let statefulsets: Api<StatefulSet> = Api::namespaced(ctx.client.clone(), &namespace);
    let secrets: Api<Secret> = Api::namespaced(ctx.client.clone(), &namespace);
    let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(ctx.client.clone(), &namespace);
    let storage_classes: Api<StorageClass> = Api::all(ctx.client.clone());
    let pdbs: Api<PodDisruptionBudget> = Api::namespaced(ctx.client.clone(), &namespace);
    let clusters: Api<FlussCluster> = Api::namespaced(ctx.client.clone(), &namespace);
    let deployments: Api<Deployment> = Api::namespaced(ctx.client.clone(), &namespace);
    let ingresses: Api<Ingress> = Api::namespaced(ctx.client.clone(), &namespace);

    let mut observations = Vec::new();
    observations
        .push(reconcilers::coordinator_service::reconcile(&services, &cluster, &uid).await?);
    observations.push(reconcilers::tablet_service::reconcile(&services, &cluster, &uid).await?);
    observations.push(reconcilers::client_service::reconcile(&services, &cluster, &uid).await?);
    observations.extend(reconcilers::config_map::reconcile(&configmaps, &cluster, &uid).await?);
    // Guardrails gate the workloads: a blocked topology, oversized heap or
    // missing storage dependency refuses pods instead of merely reporting
    // them afterwards. Services and ConfigMaps still converge first — they
    // are harmless leaves and their state feeds `.status`.
    if let Some(topology) = guardrails::replication::check(&cluster) {
        observations.push(topology);
    }
    observations.extend(guardrails::resources::check(&cluster));
    observations.extend(guardrails::storage::check(&ctx.client, &namespace, &cluster).await?);
    let workloads_blocked = observations.iter().any(|o| o.blocked_guardrail().is_some());
    if !workloads_blocked {
        observations.extend(
            reconcilers::statefulset::reconcile(
                &statefulsets,
                &secrets,
                &pvcs,
                &storage_classes,
                &cluster,
                &uid,
            )
            .await?,
        );
        observations
            .extend(reconcilers::pod_disruption_budget::reconcile(&pdbs, &cluster, &uid).await?);
    }
    // Optional Gateway: converges (or garbage-collects) the Deployment,
    // Service and Ingress after the workloads, reporting blocks the same
    // way. Disabled clusters only emit absence.
    observations.extend(
        reconcilers::gateway::reconcile(
            &deployments,
            &services,
            &ingresses,
            &secrets,
            &cluster,
            &uid,
        )
        .await?,
    );
    // Observe-only, always best-effort: health never gates, never errors,
    // and rate-limits itself. Runs even when workloads are blocked — old
    // pods from a previous good state may still answer.
    observations.extend(fluss::probe(&cluster, &ctx.probes).await);
    // Dynamic config rides the fresh probe above: same pass, same health
    // gate, and its observations land in the status write below.
    let dynamic = reconcilers::dynamic_config::reconcile(&cluster, &observations).await;
    observations.extend(dynamic);

    if reconcilers::status::reconcile(&clusters, &cluster, &observations).await? {
        tracing::info!(cluster = %name, "updated FlussCluster status");
    }

    if let Some(error) = terminal_error(&observations) {
        return Err(error);
    }
    // Periodic resync: Secret rotation changes no watched object, so without
    // a heartbeat the staleness detector would sleep until the next
    // unrelated event. Steady state is write-free, so the cost is a few
    // reads per minute per cluster.
    Ok(Action::requeue(Duration::from_secs(60)))
}

/// Translate block observations into pass-ending errors, preserving
/// priority order: static blocks park on the next watch event, while
/// blocks awaiting external change retry on a timer (see `error_policy`).
fn terminal_error(observations: &[Observation]) -> Option<Error> {
    if let Some(svc) = observations.iter().find_map(Observation::blocked_service) {
        return Some(Error::NotOwned(svc.to_string()));
    }
    if let Some((_, message)) = observations.iter().find_map(Observation::blocked_config) {
        return Some(Error::InvalidConfig(message.to_string()));
    }
    if let Some((name, _)) = observations
        .iter()
        .find_map(Observation::blocked_statefulset)
    {
        return Some(Error::NotOwnedResource {
            kind: "statefulset".to_string(),
            name: name.to_string(),
        });
    }
    if let Some((name, message)) = observations
        .iter()
        .find_map(Observation::blocked_awaiting_external)
    {
        return Some(Error::RetryableBlock {
            name: name.to_string(),
            message: message.to_string(),
        });
    }
    if let Some((name, _)) = observations.iter().find_map(Observation::blocked_pdb) {
        return Some(Error::NotOwnedResource {
            kind: "poddisruptionbudget".to_string(),
            name: name.to_string(),
        });
    }
    if let Some((_, message)) = observations.iter().find_map(Observation::blocked_volume) {
        return Some(Error::InvalidConfig(message.to_string()));
    }
    if let Some(message) = observations.iter().find_map(Observation::blocked_guardrail) {
        return Some(Error::InvalidConfig(message.to_string()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::terminal_error;
    use super::{Error, Observation};

    fn blocked_sts() -> Observation {
        Observation::StatefulSetBlocked {
            name: "tablet".to_string(),
            message: "foreign owner".to_string(),
        }
    }

    fn awaiting_external() -> Observation {
        Observation::StatefulSetAwaitingExternal {
            name: "tablet".to_string(),
            message: "ts-2 still hosts 1 replicas".to_string(),
        }
    }

    #[test]
    fn static_statefulset_block_parks() {
        match terminal_error(&[blocked_sts()]) {
            Some(Error::NotOwnedResource { kind, name }) => {
                assert_eq!(kind, "statefulset");
                assert_eq!(name, "tablet");
            }
            other => panic!("static block must park, got: {other:?}"),
        }
    }

    #[test]
    fn awaiting_external_block_retries() {
        match terminal_error(&[awaiting_external()]) {
            Some(Error::RetryableBlock { name, message }) => {
                assert_eq!(name, "tablet");
                assert!(message.contains("ts-2"), "reason rides along: {message}");
            }
            other => panic!("dynamic block must retry, got: {other:?}"),
        }
    }

    #[test]
    fn static_block_wins_over_retryable() {
        // Priority order preserved: a foreign owner parks even when the
        // gate also refuses.
        match terminal_error(&[awaiting_external(), blocked_sts()]) {
            Some(Error::NotOwnedResource { .. }) => {}
            other => panic!("static block must win, got: {other:?}"),
        }
    }

    #[test]
    fn no_block_means_no_error() {
        assert!(terminal_error(&[]).is_none());
    }
}
