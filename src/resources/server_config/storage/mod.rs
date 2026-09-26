use std::collections::BTreeMap;

use crate::api::FlussCluster;
use crate::constants::DEFAULT_DATA_DIR;

pub mod backends;

/// Render the shared storage-related `server.yaml` properties.
///
/// - `remote.data.dir`: the `s3://<bucket>/<prefix>` location (singular:
///   the 1.0.0 image ignores the plural `remote.data.dirs`).
/// - `s3.region`, `s3.endpoint`, `s3.path-style-access`: S3 access details.
/// - Secret markers (`config.providers`, `s3.access-key`, `s3.secret-key`
///   as `${directory:...}` references, never values) on the `secret`
///   authentication branch; omitted on `workloadIdentity`.
///
/// `data.dir` is deliberately not here: per the Fluss 1.0 configuration
/// reference it is a TabletServer-scoped setting, so it lives in
/// [`data_dir()`] and only the tablet assembly calls it.
pub(crate) fn properties(cluster: &FlussCluster) -> BTreeMap<String, String> {
    // Single backend today: direct call. When the second backend arrives,
    // this becomes the `match` on the backend enum, with one arm per
    // `backends::*` module.
    backends::s3::properties(&cluster.spec.remote_storage.s3)
}

/// Key resolved here that users must not override: the data directory
/// must match the StatefulSet volume mount.
pub(crate) const PROTECTED_KEYS: &[&str] = &["data.dir"];

/// Resolve the tablet data directory.
///
/// `storage.data_dir` when the CR sets it, otherwise [`DEFAULT_DATA_DIR`].
/// Tablet-only: the Fluss 1.0 reference documents `data.dir` under
/// TabletServer, while the CoordinatorServer section has no local storage
/// keys (coordinator metadata lives in ZooKeeper).
pub(crate) fn data_dir(cluster: &FlussCluster) -> String {
    cluster
        .spec
        .tablet_servers
        .storage
        .data_dir
        .clone()
        .unwrap_or(DEFAULT_DATA_DIR.to_string())
}
