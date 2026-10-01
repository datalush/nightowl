// SPDX-License-Identifier: AGPL-3.0-only
//! Remove owned routing leaves only after a scaled-in pod is actually gone.

use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{Pod, Service};
use kube::api::{ApiResource, DynamicObject, GroupVersionKind, ListParams};
use kube::{Api, Client, ResourceExt};

use super::Observation;
use crate::api::FlussCluster;
use crate::controller::{Error, apply, guardrails::ownership::owned_by};

pub async fn reconcile(
    client: &Client,
    namespace: &str,
    cluster: &FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    if cluster
        .spec
        .listeners
        .as_ref()
        .and_then(|l| l.external.as_ref())
        .is_none()
    {
        return Ok(vec![]);
    }
    let cluster_name = cluster.metadata.name.as_deref().ok_or(Error::MissingName)?;
    let routes: Api<DynamicObject> = Api::namespaced_with(
        client.clone(),
        namespace,
        &ApiResource::from_gvk(&GroupVersionKind::gvk(
            "gateway.networking.k8s.io",
            "v1",
            "TLSRoute",
        )),
    );
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let statefulsets: Api<StatefulSet> = Api::namespaced(client.clone(), namespace);
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let mut observations = Vec::new();

    // Only our controlled per-server Services qualify. The scale-in gate already
    // checked Fluss membership before changing StatefulSet replicas; checking the
    // live spec AND pod absence prevents pruning while that change is blocked.
    for service in services
        .list(&ListParams::default().labels(&format!("fluss.datalush.com/cluster={cluster_name}")))
        .await
        .map_err(Error::Kube)?
        .items
    {
        if !owned_by(&service, uid) {
            continue;
        }
        let service_name = service.name_any();
        let Some((role, ordinal)) = retired_identity(cluster_name, &service_name) else {
            continue;
        };
        let desired_replicas = if role == "coordinator" {
            cluster.spec.coordinator.replicas
        } else {
            cluster.spec.tablet_servers.replicas
        };
        if ordinal < desired_replicas {
            continue;
        }
        let sts_name = format!("{cluster_name}-{role}");
        let live = statefulsets.get(&sts_name).await.map_err(Error::Kube)?;
        if live.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0) > desired_replicas {
            continue;
        }
        let pod = format!("{sts_name}-{ordinal}");
        if pods.get_opt(&pod).await.map_err(Error::Kube)?.is_some() {
            continue;
        }
        let route = format!("{cluster_name}-native-{pod}");
        let outcome = apply::ensure_absent(&routes, &route, uid).await?;
        observations.push(Observation::GatewayConverged {
            name: route,
            outcome,
            available: None,
        });
        let outcome = apply::ensure_absent(&services, &service_name, uid).await?;
        observations.push(Observation::ServiceConverged {
            name: service_name,
            outcome,
        });
    }
    Ok(observations)
}

/// Only per-server routing names qualify; shared Services and ConfigMaps never do.
fn retired_identity(cluster: &str, service: &str) -> Option<(&'static str, i32)> {
    let body = service.strip_suffix("-external")?;
    for (prefix, role) in [
        ("coordinator", "coordinator"),
        ("tabletserver", "tabletserver"),
    ] {
        if let Some(raw) = body.strip_prefix(&format!("{cluster}-{prefix}-")) {
            return raw.parse().ok().map(|ordinal| (role, ordinal));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_owned_shape_is_eligible() {
        assert_eq!(
            retired_identity("native", "native-tabletserver-2-external"),
            Some(("tabletserver", 2))
        );
        assert_eq!(
            retired_identity("native", "native-coordinator-1-external"),
            Some(("coordinator", 1))
        );
        for name in [
            "native-bootstrap",
            "foreign-tabletserver-2-external",
            "native-tabletserver-no-external",
        ] {
            assert!(retired_identity("native", name).is_none());
        }
    }
}
