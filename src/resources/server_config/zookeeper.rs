use std::collections::BTreeMap;

use crate::api::FlussCluster;

/// Render the ZooKeeper-related `server.yaml` properties.
///
/// - `zookeeper.address`: the spec addresses joined with commas, which is
///   what Fluss expects.
/// - `zookeeper.path.root`: `pathRoot` when the CR sets it, otherwise a
///   stable default derived from the cluster identity.
pub(crate) fn properties(cluster: &FlussCluster) -> BTreeMap<String, String> {
    let path_root = cluster.spec.zookeeper.path_root.clone().unwrap_or_else(|| {
        let namespace = cluster
            .metadata
            .namespace
            .clone()
            .expect("FlussCluster needs a namespace");
        let name = cluster
            .metadata
            .name
            .clone()
            .expect("FlussCluster needs a name");
        format!("/fluss/{namespace}/{name}")
    });

    BTreeMap::from([
        ("zookeeper.path.root".to_string(), path_root),
        (
            "zookeeper.address".to_string(),
            cluster.spec.zookeeper.addresses.join(","),
        ),
    ])
}
