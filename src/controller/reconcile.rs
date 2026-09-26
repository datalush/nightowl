use std::sync::Arc;
use std::time::Duration;

use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{ConfigMap, Secret, Service};
use k8s_openapi::api::policy::v1::PodDisruptionBudget;
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
    let pdbs: Api<PodDisruptionBudget> = Api::namespaced(ctx.client.clone(), &namespace);
    let clusters: Api<FlussCluster> = Api::namespaced(ctx.client.clone(), &namespace);

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
            reconcilers::statefulset::reconcile(&statefulsets, &secrets, &cluster, &uid).await?,
        );
        observations
            .extend(reconcilers::pod_disruption_budget::reconcile(&pdbs, &cluster, &uid).await?);
    }
    // Observe-only, always best-effort: health never gates, never errors,
    // and rate-limits itself. Runs even when workloads are blocked — old
    // pods from a previous good state may still answer.
    observations.extend(fluss::probe(&cluster, &ctx.probes).await);

    if reconcilers::status::reconcile(&clusters, &cluster, &observations).await? {
        tracing::info!(cluster = %name, "updated FlussCluster status");
    }

    if let Some(svc) = observations.iter().find_map(Observation::blocked_service) {
        return Err(Error::NotOwned(svc.to_string()));
    }
    if let Some((_, message)) = observations.iter().find_map(Observation::blocked_config) {
        return Err(Error::InvalidConfig(message.to_string()));
    }
    if let Some((name, _)) = observations
        .iter()
        .find_map(Observation::blocked_statefulset)
    {
        return Err(Error::NotOwnedResource {
            kind: "statefulset".to_string(),
            name: name.to_string(),
        });
    }
    if let Some((name, _)) = observations.iter().find_map(Observation::blocked_pdb) {
        return Err(Error::NotOwnedResource {
            kind: "poddisruptionbudget".to_string(),
            name: name.to_string(),
        });
    }
    if let Some(message) = observations.iter().find_map(Observation::blocked_guardrail) {
        return Err(Error::InvalidConfig(message.to_string()));
    }
    // Periodic resync: Secret rotation changes no watched object, so without
    // a heartbeat the staleness detector would sleep until the next
    // unrelated event. Steady state is write-free, so the cost is a few
    // reads per minute per cluster.
    Ok(Action::requeue(Duration::from_secs(60)))
}
