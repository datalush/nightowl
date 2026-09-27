// SPDX-License-Identifier: AGPL-3.0-only
mod apply;
mod fluss;
mod guardrails;
mod reconcile;
mod reconcilers;

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{ConfigMap, Service};
use k8s_openapi::api::policy::v1::PodDisruptionBudget;
use kube::runtime::controller::Action;
use kube::runtime::{Controller, watcher};
use kube::{Api, Client};

pub use reconcile::reconcile;

use crate::api::FlussCluster;

/// Shared state for every reconciliation.
pub struct Context {
    pub client: Client,
    pub probes: fluss::ProbeClock,
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
/// instead of hot-looping. API rejections (the apiserver refusing an
/// invalid or forbidden write) are static the same way. Only genuinely
/// transient API errors requeue with a fixed delay.
fn error_policy(cluster: Arc<FlussCluster>, error: &Error, _ctx: Arc<Context>) -> Action {
    let name = cluster.metadata.name.as_deref().unwrap_or("<no-name>");
    match error {
        Error::NotOwned(_) | Error::NotOwnedResource { .. } | Error::InvalidConfig(_) => {
            tracing::error!(cluster = %name, error = %error, "reconcile blocked");
            Action::await_change()
        }
        Error::Kube(kube::Error::Api(status))
            if matches!(status.code, 400 | 403 | 404 | 405 | 409 | 422) =>
        {
            tracing::error!(cluster = %name, error = %error, "reconcile rejected");
            Action::await_change()
        }
        _ => {
            tracing::warn!(cluster = %name, error = %error, "reconcile failed, retrying");
            Action::requeue(Duration::from_secs(10))
        }
    }
}

/// One namespace or all of them, per the `--namespace` flag.
fn scoped<T>(client: Client, namespace: &Option<String>) -> Api<T>
where
    T: kube::Resource<Scope = kube::core::NamespaceResourceScope>,
    T::DynamicType: Default,
{
    match namespace {
        Some(namespace) => Api::namespaced(client, namespace),
        None => Api::all(client),
    }
}

/// Run the FlussCluster controller until the stream ends.
///
/// With `Some(namespace)` watches that namespace only; with `None` watches
/// all namespaces. Per-object reconciliation already keys off the object's
/// own namespace, so only the watch scope changes here. Watches
/// FlussCluster objects and the resources they own, so deleting a managed
/// object triggers a new reconciliation that recreates it.
pub async fn run(client: Client, namespace: Option<String>) {
    let clusters: Api<FlussCluster> = scoped(client.clone(), &namespace);
    let services: Api<Service> = scoped(client.clone(), &namespace);
    let configmaps: Api<ConfigMap> = scoped(client.clone(), &namespace);
    let statefulsets: Api<StatefulSet> = scoped(client.clone(), &namespace);
    let pdbs: Api<PodDisruptionBudget> = scoped(client.clone(), &namespace);
    let context = Arc::new(Context {
        client,
        probes: fluss::ProbeClock::default(),
    });

    Controller::new(clusters, watcher::Config::default())
        .owns(services, watcher::Config::default())
        .owns(configmaps, watcher::Config::default())
        .owns(statefulsets, watcher::Config::default())
        .owns(pdbs, watcher::Config::default())
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

#[cfg(test)]
mod deploy_tests {
    use std::collections::BTreeSet;

    /// The checked-in manifests must grant exactly the verbs the controller
    /// uses — no more, no less. Widen the code (a new delete path, a new
    /// watched kind) and this test forces the manifests to follow
    /// consciously, in the same commit.
    #[test]
    fn clusterrole_grants_exactly_the_verbs_the_controller_uses() {
        let role: k8s_openapi::api::rbac::v1::ClusterRole =
            serde_yaml::from_str(include_str!("../../deploy/clusterrole.yaml"))
                .expect("clusterrole must parse");
        let rules = role.rules.expect("clusterrole needs rules");
        assert_eq!(rules.len(), 10, "one rule per row of the verb matrix");

        let mut remaining: Vec<(Vec<String>, Vec<String>, Vec<String>)> = rules
            .iter()
            .map(|rule| {
                (
                    sorted(rule.api_groups.as_deref().unwrap_or(&[])),
                    sorted(rule.resources.as_deref().unwrap_or(&[])),
                    sorted(&rule.verbs),
                )
            })
            .collect();
        for (groups, resources, verbs) in [
            (
                vec!["fluss.datalush.com"],
                vec!["flussclusters"],
                vec!["get", "list", "watch"],
            ),
            (
                vec!["fluss.datalush.com"],
                vec!["flussclusters/status"],
                vec!["update"],
            ),
            (
                vec![""],
                vec!["configmaps", "services"],
                vec![
                    "create", "delete", "get", "list", "patch", "update", "watch",
                ],
            ),
            (
                vec!["apps"],
                vec!["statefulsets"],
                vec!["create", "get", "list", "patch", "update", "watch"],
            ),
            (
                vec!["apps"],
                vec!["deployments"],
                vec![
                    "create", "delete", "get", "list", "patch", "update", "watch",
                ],
            ),
            (
                vec!["networking.k8s.io"],
                vec!["ingresses"],
                vec![
                    "create", "delete", "get", "list", "patch", "update", "watch",
                ],
            ),
            (
                vec!["policy"],
                vec!["poddisruptionbudgets"],
                vec![
                    "create", "delete", "get", "list", "patch", "update", "watch",
                ],
            ),
            (
                vec![""],
                vec!["persistentvolumeclaims"],
                vec!["list", "patch"],
            ),
            (
                vec!["storage.k8s.io"],
                vec!["storageclasses"],
                vec!["get", "list"],
            ),
            (vec![""], vec!["secrets", "serviceaccounts"], vec!["get"]),
        ] {
            let position = remaining.iter().position(|(g, r, v)| {
                g == &groups.iter().map(|s| s.to_string()).collect::<Vec<_>>()
                    && r == &resources.iter().map(|s| s.to_string()).collect::<Vec<_>>()
                    && v == &verbs.iter().map(|s| s.to_string()).collect::<Vec<_>>()
            });
            assert!(
                position.is_some(),
                "missing rule for {groups:?} {resources:?} {verbs:?}"
            );
            remaining.remove(position.expect("checked above"));
        }
        assert!(remaining.is_empty(), "no extra rules: {remaining:?}");
    }

    #[test]
    fn binding_and_workload_point_at_the_same_service_account() {
        let binding: k8s_openapi::api::rbac::v1::ClusterRoleBinding =
            serde_yaml::from_str(include_str!("../../deploy/clusterrolebinding.yaml"))
                .expect("binding must parse");
        assert_eq!(binding.role_ref.name, "nightowl");
        let subjects = binding.subjects.expect("binding needs subjects");
        assert_eq!(subjects.len(), 1);
        assert_eq!(subjects[0].name, "nightowl");
        assert_eq!(
            subjects[0].namespace.as_deref(),
            Some("operator-system"),
            "least privilege starts with naming the right namespace"
        );

        let deployment: k8s_openapi::api::apps::v1::Deployment =
            serde_yaml::from_str(include_str!("../../deploy/deployment.yaml"))
                .expect("deployment must parse");
        assert_eq!(
            deployment
                .spec
                .as_ref()
                .expect("deployment needs a spec")
                .template
                .spec
                .as_ref()
                .expect("pod template needs a spec")
                .service_account_name,
            Some("nightowl".to_string()),
            "the pod must run as the bound account, not the default one"
        );
    }

    fn sorted(values: &[String]) -> Vec<String> {
        values
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}
