use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

use crate::api::FlussCluster;
use crate::constants::{
    API_VERSION, CONFIG_DATA_KEY, COORDINATOR_CONFIG_SUFFIX, KIND_FLUSS_CLUSTER, LABEL_CLUSTER,
    LABEL_ROLE, ROLE_COORDINATOR, ROLE_TABLET, TABLET_CONFIG_SUFFIX,
};
use crate::utils::render;

use super::server_config;

pub fn desired_coordinator_config(cluster: &FlussCluster) -> ConfigMap {
    let name = cluster
        .metadata
        .name
        .clone()
        .expect("FlussCluster needs a name");

    let cm_name = format!("{}{}", name, COORDINATOR_CONFIG_SUFFIX);

    let namespace = cluster.metadata.namespace.clone();

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
        name: Some(cm_name),
        namespace,
        labels: Some(labels),
        owner_references: Some(vec![owner_ref]),
        ..Default::default()
    };

    let mut properties = server_config::zookeeper::properties(cluster);
    properties.extend(server_config::listeners::properties(cluster));
    properties.extend(server_config::storage::properties(cluster));

    let server_yaml = render::to_yaml(&properties);
    let data = BTreeMap::from([(CONFIG_DATA_KEY.to_string(), server_yaml)]);

    ConfigMap {
        metadata: object_meta,
        data: Some(data),
        binary_data: None,
        immutable: None,
    }
}

pub fn desired_tablet_config(cluster: &FlussCluster) -> ConfigMap {
    let name = cluster
        .metadata
        .name
        .clone()
        .expect("FlussCluster needs a name");

    let cm_name = format!("{}{}", name, TABLET_CONFIG_SUFFIX);

    let namespace = cluster.metadata.namespace.clone();

    let labels = BTreeMap::from([
        (LABEL_CLUSTER.to_string(), name.clone()),
        (LABEL_ROLE.to_string(), ROLE_TABLET.to_string()),
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
        name: Some(cm_name),
        namespace,
        labels: Some(labels),
        owner_references: Some(vec![owner_ref]),
        ..Default::default()
    };

    let mut properties = server_config::zookeeper::properties(cluster);
    properties.extend(server_config::listeners::properties(cluster));
    properties.extend(server_config::storage::properties(cluster));
    properties.insert(
        "data.dir".to_string(),
        server_config::storage::data_dir(cluster),
    );

    let server_yaml = render::to_yaml(&properties);
    let data = BTreeMap::from([(CONFIG_DATA_KEY.to_string(), server_yaml)]);

    ConfigMap {
        metadata: object_meta,
        data: Some(data),
        binary_data: None,
        immutable: None,
    }
}
