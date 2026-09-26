use std::collections::BTreeMap;

use super::{listeners, storage, table_defaults, zookeeper};

/// Rejected user override: the key is owned by the Operator.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("override of operator-owned key {0} is forbidden")]
pub struct ForbiddenKey(pub String);

/// Keys the Operator controls but that never appear in the base map because
/// they are composed per Pod at boot, not rendered into the shared config.
const RESERVED_ABSENT_KEYS: &[&str] =
    &["bind.listeners", "advertised.listeners", "tablet-server.id"];

/// True when users must not set the key: rendered as protected by some
/// module, or reserved despite being absent from the base map.
fn is_protected(key: &str) -> bool {
    RESERVED_ABSENT_KEYS.contains(&key)
        || zookeeper::PROTECTED_KEYS.contains(&key)
        || listeners::PROTECTED_KEYS.contains(&key)
        || storage::PROTECTED_KEYS.contains(&key)
        || storage::backends::s3::PROTECTED_KEYS.contains(&key)
        || table_defaults::PROTECTED_KEYS.contains(&key)
}

/// Merge user overrides over the Operator-rendered base properties.
///
/// Precedence is fixed: `base_properties` < `cluster_overrides` <
/// `component_overrides`, so a component-level override wins over a
/// cluster-wide one. Keys owned by the Operator
/// (identity, topology, credentials, rendered storage and defaults) are
/// rejected with [`ForbiddenKey`] before anything merges — fail closed,
/// never partially applied.
pub(crate) fn apply(
    base_properties: BTreeMap<String, String>,
    cluster_overrides: &BTreeMap<String, String>,
    component_overrides: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, ForbiddenKey> {
    for key in cluster_overrides.keys().chain(component_overrides.keys()) {
        if is_protected(key) {
            return Err(ForbiddenKey(key.clone()));
        }
    }
    let mut merged = base_properties;
    merged.extend(cluster_overrides.clone());
    merged.extend(component_overrides.clone());
    Ok(merged)
}
