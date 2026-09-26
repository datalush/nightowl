use std::sync::Arc;

use k8s_openapi::api::core::v1::Service;
use kube::Api;
use kube::runtime::controller::Action;

use super::{Context, Error};
use crate::api::FlussCluster;
use crate::resources::coordinator_service;

/// Reconcile one FlussCluster: converge its Coordinator headless Service.
///
/// - Missing Service -> create it.
/// - Existing Service owned by us with a stale spec -> update it.
/// - Existing Service with a different owner -> refuse to adopt it.
/// - Identical Service -> do nothing.
pub async fn reconcile(cluster: Arc<FlussCluster>, ctx: Arc<Context>) -> Result<Action, Error> {
    let name = cluster.metadata.name.clone().ok_or(Error::MissingName)?;
    let namespace = cluster
        .metadata
        .namespace
        .clone()
        .ok_or(Error::MissingNamespace)?;
    let uid = cluster.metadata.uid.clone().ok_or(Error::MissingUid)?;

    let desired = coordinator_service::desired_service(&cluster);
    let desired_name = desired.metadata.name.clone().ok_or(Error::MissingName)?;

    let services: Api<Service> = Api::namespaced(ctx.client.clone(), &namespace);
    match services.get(&desired_name).await {
        Err(kube::Error::Api(status)) if status.code == 404 => {
            tracing::info!(
                service = %desired_name,
                cluster = %name,
                "creating coordinator service"
            );
            services
                .create(&kube::api::PostParams::default(), &desired)
                .await
                .map_err(Error::Kube)?;
        }
        Err(e) => return Err(Error::Kube(e)),
        Ok(existing) => {
            if !owned_by(&existing, &uid) {
                return Err(Error::NotOwned(desired_name));
            }
            if existing.spec != desired.spec {
                // replace() needs the current resourceVersion or the API
                // rejects it with a 409 conflict.
                let mut to_update = desired;
                to_update.metadata.resource_version = existing.metadata.resource_version.clone();
                tracing::info!(
                    service = %desired_name,
                    cluster = %name,
                    "updating coordinator service"
                );
                services
                    .replace(&desired_name, &kube::api::PostParams::default(), &to_update)
                    .await
                    .map_err(Error::Kube)?;
            }
        }
    }

    Ok(Action::await_change())
}

/// True when the existing Service is controlled by our FlussCluster.
///
/// Ownership is identified by uid, not by name: a deleted and recreated
/// FlussCluster keeps its name but gets a fresh uid, and must not inherit
/// the previous incarnation's resources silently.
fn owned_by(existing: &Service, uid: &str) -> bool {
    existing
        .metadata
        .owner_references
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|owner| owner.uid == uid && owner.controller == Some(true))
}
