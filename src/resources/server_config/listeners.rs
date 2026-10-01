// SPDX-License-Identifier: AGPL-3.0-only
use std::collections::BTreeMap;

use crate::api::FlussCluster;

/// Render the listener-related `server.yaml` properties.
///
/// - `internal.listener.name`: the internal listener name from the CR.
///   Only the internal name belongs in the shared config; per-pod
///   `bind.listeners` / `advertised.listeners` are composed at boot.
///
/// Key rendered here that users must not override: the internal
/// listener name is owned by the Operator.
pub(crate) const PROTECTED_KEYS: &[&str] = &["internal.listener.name"];

pub(crate) fn properties(cluster: &FlussCluster) -> BTreeMap<String, String> {
    let name = cluster.spec.resolved_listeners().internal.name.clone();

    BTreeMap::from([("internal.listener.name".to_string(), name)])
}
