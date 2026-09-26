mod apply;
mod guardrails;
mod reconcile;
mod reconcilers;

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{ConfigMap, Service};
use kube::runtime::controller::Action;
use kube::runtime::{Controller, watcher};
use kube::{Api, Client};

pub use reconcile::reconcile;

use crate::api::FlussCluster;

/// Shared state for every reconciliation.
pub struct Context {
    pub client: Client,
}

/// Reconciler failures.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("FlussCluster is missing metadata.name")]
    MissingName,
    #[error("FlussCluster is missing metadata.namespace")]
    MissingNamespace,
    #[error("FlussCluster is missing metadata.uid")]
    MissingUid,
    #[error("service {0} exists with a different owner; refusing to adopt it")]
    NotOwned(String),
    #[error("{kind} {name} exists with a different owner; refusing to adopt it")]
    NotOwnedResource { kind: String, name: String },
    #[error("invalid config: {0}")]
    InvalidConfig(String),
    #[error("kubernetes api error: {0}")]
    Kube(#[source] kube::Error),
}

/// Decide what to do after a failed reconciliation.
///
/// Static user errors (a conflicting owner, an invalid config) will not
/// resolve themselves by retrying, so they wait for the next watch event
/// instead of hot-looping. Transient API errors requeue with a fixed delay.
fn error_policy(cluster: Arc<FlussCluster>, error: &Error, _ctx: Arc<Context>) -> Action {
    let name = cluster.metadata.name.as_deref().unwrap_or("<no-name>");
    match error {
        Error::NotOwned(_) | Error::NotOwnedResource { .. } | Error::InvalidConfig(_) => {
            tracing::error!(cluster = %name, error = %error, "reconcile blocked");
            Action::await_change()
        }
        _ => {
            tracing::warn!(cluster = %name, error = %error, "reconcile failed, retrying");
            Action::requeue(Duration::from_secs(10))
        }
    }
}

/// Run the FlussCluster controller in `namespace` until the stream ends.
///
/// Watches FlussCluster objects and the Services they own, so deleting a
/// managed Service triggers a new reconciliation that recreates it.
pub async fn run(client: Client, namespace: &str) {
    let clusters: Api<FlussCluster> = Api::namespaced(client.clone(), namespace);
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let configmaps: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let statefulsets: Api<StatefulSet> = Api::namespaced(client.clone(), namespace);
    let context = Arc::new(Context { client });

    Controller::new(clusters, watcher::Config::default())
        .owns(services, watcher::Config::default())
        .owns(configmaps, watcher::Config::default())
        .owns(statefulsets, watcher::Config::default())
        .run(reconcile, error_policy, context)
        .for_each(|result| async move {
            match result {
                Ok((obj, _)) => {
                    tracing::debug!(name = %obj.name, "reconciled");
                }
                Err(e) => {
                    tracing::error!(error = %e, "reconcile stream error");
                }
            }
        })
        .await;
}
