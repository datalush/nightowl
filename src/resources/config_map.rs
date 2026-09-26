use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ConfigMap;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};

use crate::api::FlussCluster;
use crate::constants::{
    API_VERSION, CONFIG_DATA_KEY, CONFIG_HASH_ANNOTATION, COORDINATOR_CONFIG_SUFFIX,
    KIND_FLUSS_CLUSTER, LABEL_CLUSTER, LABEL_ROLE, ROLE_COORDINATOR, ROLE_TABLET,
    TABLET_CONFIG_SUFFIX,
};
use crate::utils::{hash, render};

use super::server_config;

pub fn desired_coordinator_config(
    cluster: &FlussCluster,
) -> Result<ConfigMap, server_config::ConfigError> {
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

    let mut properties = server_config::zookeeper::properties(cluster);
    properties.extend(server_config::listeners::properties(cluster));
    properties.extend(server_config::storage::properties(cluster));
    properties.extend(server_config::table_defaults::properties(cluster));
    let mut properties = server_config::overrides::apply(
        properties,
        &cluster.spec.configuration_overrides,
        &cluster.spec.coordinator.configuration_overrides,
    )?;
    server_config::table_defaults::ensure_quorum(&mut properties, cluster.spec.defaults.as_ref())?;

    let server_yaml = render::to_yaml(&properties);
    let config_hash = hash::sha256_hex(&server_yaml);
    let data = BTreeMap::from([(CONFIG_DATA_KEY.to_string(), server_yaml)]);

    let object_meta = ObjectMeta {
        name: Some(cm_name),
        namespace,
        labels: Some(labels),
        annotations: Some(BTreeMap::from([(
            CONFIG_HASH_ANNOTATION.to_string(),
            config_hash,
        )])),
        owner_references: Some(vec![owner_ref]),
        ..Default::default()
    };

    Ok(ConfigMap {
        metadata: object_meta,
        data: Some(data),
        binary_data: None,
        immutable: None,
    })
}

pub fn desired_tablet_config(
    cluster: &FlussCluster,
) -> Result<ConfigMap, server_config::ConfigError> {
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

    let mut properties = server_config::zookeeper::properties(cluster);
    properties.extend(server_config::listeners::properties(cluster));
    properties.extend(server_config::storage::properties(cluster));
    properties.extend(server_config::table_defaults::properties(cluster));
    properties.insert(
        "data.dir".to_string(),
        server_config::storage::data_dir(cluster),
    );
    let mut properties = server_config::overrides::apply(
        properties,
        &cluster.spec.configuration_overrides,
        &cluster.spec.tablet_servers.configuration_overrides,
    )?;
    server_config::table_defaults::ensure_quorum(&mut properties, cluster.spec.defaults.as_ref())?;

    let server_yaml = render::to_yaml(&properties);
    let config_hash = hash::sha256_hex(&server_yaml);
    let data = BTreeMap::from([(CONFIG_DATA_KEY.to_string(), server_yaml)]);

    let object_meta = ObjectMeta {
        name: Some(cm_name),
        namespace,
        labels: Some(labels),
        annotations: Some(BTreeMap::from([(
            CONFIG_HASH_ANNOTATION.to_string(),
            config_hash,
        )])),
        owner_references: Some(vec![owner_ref]),
        ..Default::default()
    };

    Ok(ConfigMap {
        metadata: object_meta,
        data: Some(data),
        binary_data: None,
        immutable: None,
    })
}
