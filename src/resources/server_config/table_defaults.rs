use std::collections::BTreeMap;

use crate::api::FlussCluster;

/// Render the table-default `server.yaml` properties.
///
/// - `default.bucket.number` and `default.replication.factor` from the
///   cluster-wide `defaults` section when the CR sets it; absent entirely
///   otherwise, letting the Fluss server defaults apply.
/// - `log.replica.min-in-sync-replicas-number`: the explicit
///   `min_in_sync_replicas` value when set, otherwise the quorum default
///   `floor(log_replication_factor / 2) + 1` (1 when RF is 1, no special
///   case). Verified against the Fluss 1.0 configuration reference and the
///   absence of any per-table min-ISR property in `TableConfig`: it is a
///   server-level log write-durability setting, not a per-table default,
///   and the schema rejects explicit values above the replication factor.
pub(crate) fn properties(cluster: &FlussCluster) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    if let Some(defaults) = cluster.spec.defaults.as_ref() {
        props.insert(
            "default.bucket.number".to_string(),
            defaults.buckets.to_string(),
        );
        props.insert(
            "default.replication.factor".to_string(),
            defaults.log_replication_factor.to_string(),
        );
        let min_in_sync = defaults.min_in_sync_replicas.unwrap_or_else(|| {
            let rf = defaults.log_replication_factor;
            rf / 2 + 1
        });
        props.insert(
            "log.replica.min-in-sync-replicas-number".to_string(),
            min_in_sync.to_string(),
        );
    }
    props
}
