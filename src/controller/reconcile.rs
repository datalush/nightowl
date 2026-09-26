use std::sync::Arc;

use k8s_openapi::api::core::v1::{ConfigMap, Service};
use kube::Api;
use kube::runtime::controller::Action;

use super::reconcilers::{self, Observation};
use super::{Context, Error};
use crate::api::FlussCluster;

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
    let clusters: Api<FlussCluster> = Api::namespaced(ctx.client.clone(), &namespace);

    let mut observations = Vec::new();
    observations
        .push(reconcilers::coordinator_service::reconcile(&services, &cluster, &uid).await?);
    observations.extend(reconcilers::config_map::reconcile(&configmaps, &cluster, &uid).await?);

    if reconcilers::status::reconcile(&clusters, &cluster, &observations).await? {
        tracing::info!(cluster = %name, "updated FlussCluster status");
    }

    if let Some(svc) = observations.iter().find_map(Observation::blocked_service) {
        return Err(Error::NotOwned(svc.to_string()));
    }
    if let Some((_, message)) = observations.iter().find_map(Observation::blocked_config) {
        return Err(Error::InvalidConfig(message.to_string()));
    }
    Ok(Action::await_change())
}
