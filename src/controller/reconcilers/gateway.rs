//! Reconcile the optional Gateway (Deployment plus Service plus optional
//! Ingress) and report what happened.
//!
//! Disabled or absent means garbage-collect previously managed objects,
//! never adopt or leave them. A referenced TLS secret must already exist;
//! the ingress class travels verbatim (environment data, not validated).

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Secret, Service};
use k8s_openapi::api::networking::v1::Ingress;
use kube::Api;
use kube::Resource;

use super::Observation;
use crate::controller::Error;
use crate::controller::apply::{self, ApplyOutcome};
use crate::resources::gateway as builder;

/// Converge (or remove) the Gateway objects for one cluster.
pub async fn reconcile(
    deployments: &Api<Deployment>,
    services: &Api<Service>,
    ingresses: &Api<Ingress>,
    secrets: &Api<Secret>,
    cluster: &crate::api::FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    let Some(spec) = cluster.spec.gateway.as_ref() else {
        return remove_all(deployments, services, ingresses, cluster, uid).await;
    };
    if !spec.enabled {
        return remove_all(deployments, services, ingresses, cluster, uid).await;
    }
    // Fail closed before touching anything: a referenced TLS secret must
    // already exist in the cluster namespace.
    if let Some(ingress) = spec.ingress.as_ref()
        && let Some(secret) = ingress.tls_secret_name.as_ref()
        && secrets.get(secret).await.is_err()
    {
        let name = builder::ingress_name(cluster);
        return Ok(vec![Observation::GatewayBlocked {
            name,
            message: format!(
                "tls secret '{secret}' referenced by gateway ingress not found in namespace"
            ),
        }]);
    }

    let mut observations = Vec::with_capacity(3);
    let deployment = builder::desired_deployment(cluster);
    let deployment_name = deployment.meta().name.clone().ok_or(Error::MissingName)?;
    match apply::apply(deployments, deployment, uid, builder::same_deployment).await {
        Ok(outcome) => {
            let available = deployments
                .get(&deployment_name)
                .await
                .map_err(Error::Kube)?
                .status
                .and_then(|status| status.available_replicas)
                .unwrap_or(0);
            observations.push(Observation::GatewayConverged {
                name: deployment_name,
                outcome,
                available: Some(available),
            });
        }
        Err(Error::NotOwned(name)) => observations.push(Observation::GatewayBlocked {
            name: name.clone(),
            message: Error::NotOwned(name).to_string(),
        }),
        Err(error) => return Err(error),
    }

    let service = builder::desired_service(cluster);
    let service_name = service.meta().name.clone().ok_or(Error::MissingName)?;
    match apply::apply(services, service, uid, builder::same_service).await {
        Ok(outcome) => observations.push(Observation::GatewayConverged {
            name: service_name,
            outcome,
            available: None,
        }),
        Err(Error::NotOwned(name)) => observations.push(Observation::GatewayBlocked {
            name: name.clone(),
            message: Error::NotOwned(name).to_string(),
        }),
        Err(error) => return Err(error),
    }

    if spec.ingress.is_some() {
        let ingress = builder::desired_ingress(cluster);
        let ingress_name = ingress.meta().name.clone().ok_or(Error::MissingName)?;
        match apply::apply(ingresses, ingress, uid, builder::same_ingress).await {
            Ok(outcome) => observations.push(Observation::GatewayConverged {
                name: ingress_name,
                outcome,
                available: None,
            }),
            Err(Error::NotOwned(name)) => observations.push(Observation::GatewayBlocked {
                name: name.clone(),
                message: Error::NotOwned(name).to_string(),
            }),
            Err(error) => return Err(error),
        }
    } else {
        let name = builder::ingress_name(cluster);
        match apply::ensure_absent(ingresses, &name, uid).await {
            Ok(ApplyOutcome::Deleted) => observations.push(Observation::GatewayConverged {
                name,
                outcome: ApplyOutcome::Deleted,
                available: None,
            }),
            Ok(_) => {}
            Err(Error::NotOwned(name)) => observations.push(Observation::GatewayBlocked {
                name: name.clone(),
                message: Error::NotOwned(name).to_string(),
            }),
            Err(error) => return Err(error),
        }
    }

    Ok(observations)
}

/// Garbage-collect all three Gateway objects; missing ones read as
/// silence, foreign-owned ones refuse.
async fn remove_all(
    deployments: &Api<Deployment>,
    services: &Api<Service>,
    ingresses: &Api<Ingress>,
    cluster: &crate::api::FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    for (kind, name, outcome) in [
        (
            "deployment",
            builder::deployment_name(cluster),
            apply::ensure_absent(deployments, &builder::deployment_name(cluster), uid).await,
        ),
        (
            "service",
            builder::service_name(cluster),
            apply::ensure_absent(services, &builder::service_name(cluster), uid).await,
        ),
        (
            "ingress",
            builder::ingress_name(cluster),
            apply::ensure_absent(ingresses, &builder::ingress_name(cluster), uid).await,
        ),
    ] {
        match outcome {
            Ok(_) => {}
            Err(Error::NotOwned(_)) => {
                return Ok(vec![Observation::GatewayBlocked {
                    message: format!("{kind} {name} exists with a different owner"),
                    name,
                }]);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(vec![Observation::GatewayAbsent])
}
