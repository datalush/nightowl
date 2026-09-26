use crate::api::FlussCluster;
use crate::constants::{
    API_VERSION, COORDINATOR_HEADLESS_SUFFIX, KIND_FLUSS_CLUSTER, LABEL_CLUSTER, LABEL_ROLE,
    PORT_NAME_INTERNAL, ROLE_COORDINATOR,
};

use k8s_openapi::api::core::v1::{Service, ServicePort, ServiceSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use std::collections::BTreeMap;

pub fn desired_service(cluster: &FlussCluster) -> Service {
    let name = cluster
        .metadata
        .name
        .clone()
        .expect("FlussCluster needs a name");
    let namespace = cluster.metadata.namespace.clone();
    let svc_name = format!("{name}{COORDINATOR_HEADLESS_SUFFIX}");

    let labels = BTreeMap::from([
        (LABEL_CLUSTER.to_string(), name.clone()),
        (LABEL_ROLE.to_string(), ROLE_COORDINATOR.to_string()),
    ]);

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

    let object_meta = ObjectMeta {
        name: Some(svc_name),
        namespace,
        labels: Some(labels.clone()),
        owner_references: Some(vec![owner_ref]),
        ..Default::default()
    };

    let selector = labels;

    let port = cluster
        .spec
        .listeners
        .as_ref()
        .expect("listeners is required for the coordinator service")
        .internal
        .port;

    let svc_spec = ServiceSpec {
        cluster_ip: Some("None".to_string()),
        selector: Some(selector),
        ports: Some(vec![ServicePort {
            name: Some(PORT_NAME_INTERNAL.to_string()),
            port,
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
