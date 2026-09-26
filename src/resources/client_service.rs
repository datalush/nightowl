//! Shared client ClusterIP Service for in-cluster Fluss clients.
//!
//! One Service, both roles: the CR models a single client listener and Fluss
//! clients bootstrap from any endpoint. The selector carries only the cluster
//! label so coordinator (metadata) and tablet (data) pods all serve it.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

use crate::api::FlussCluster;
use crate::constants::{
    API_VERSION, CLIENT_SERVICE_SUFFIX, KIND_FLUSS_CLUSTER, LABEL_CLUSTER, PORT_NAME_CLIENT,
};

pub fn desired_service(cluster: &FlussCluster) -> Service {
    let name = cluster
        .metadata
        .name
        .clone()
        .expect("FlussCluster needs a name");
    let namespace = cluster.metadata.namespace.clone();
    let svc_name = format!("{name}{CLIENT_SERVICE_SUFFIX}");

    let labels = BTreeMap::from([(LABEL_CLUSTER.to_string(), name.clone())]);

    let uid = cluster
        .metadata
        .uid
        .clone()
        .expect("FlussCluster needs a uid");

    let owner_ref = OwnerReference {
        api_version: API_VERSION.to_string(),
        kind: KIND_FLUSS_CLUSTER.to_string(),
        name: name.clone(),
        uid,
        controller: Some(true),
        block_owner_deletion: Some(true),
    };

    let listeners = cluster
        .spec
        .listeners
        .as_ref()
        .expect("listeners is required for the client service");
    let mut annotations = BTreeMap::new();
    annotations.extend(listeners.client.annotations.clone());

    let object_meta = ObjectMeta {
        name: Some(svc_name),
        namespace,
        labels: Some(labels.clone()),
        annotations: Some(annotations),
        owner_references: Some(vec![owner_ref]),
        ..Default::default()
    };

    let svc_spec = ServiceSpec {
        selector: Some(labels),
        ports: Some(vec![ServicePort {
            name: Some(PORT_NAME_CLIENT.to_string()),
            port: listeners.client.port,
            protocol: Some("TCP".to_string()),
            ..Default::default()
        }]),
        ..Default::default()
    };

    Service {
        metadata: object_meta,
        spec: Some(svc_spec),
        status: None,
    }
}

#[cfg(test)]
mod tests {
    use super::desired_service;

    fn spike_cluster() -> crate::api::FlussCluster {
        let mut cluster: crate::api::FlussCluster =
            serde_yaml::from_str(include_str!("../../../lab2/demo.yml"))
                .expect("lab2 demo CR must deserialize");
        cluster.metadata.uid = Some("test-uid".to_string());
        cluster
    }

    #[test]
    fn client_service_selects_both_roles_on_the_client_port() {
        let svc = desired_service(&spike_cluster());
        let spec = svc.spec.as_ref().expect("service needs a spec");
        assert_eq!(
            spec.selector.as_ref().expect("service needs a selector"),
            &std::collections::BTreeMap::from([(
                "fluss.datalush.com/cluster".to_string(),
                "spike".to_string(),
            )]),
            "no role label: coordinator and tablets all serve clients"
        );
        let port = &spec.ports.as_ref().expect("service needs ports")[0];
        assert_eq!(port.name.as_deref(), Some("client"));
        assert_eq!(port.port, 9124);
        assert_eq!(spec.cluster_ip.as_deref(), None, "ClusterIP, not headless");
    }
}
