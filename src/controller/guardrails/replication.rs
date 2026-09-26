//! Static topology backstop: RF must not exceed tablet count.
//!
//! Mirrors the CEL rule on the parent spec for upgrade windows where the
//! installed CRD predates it (our own docs warn old CRDs lack new rules
//! until updated). Pure function over the spec: no API calls, no state.

use super::super::reconcilers::Observation;
use crate::api::FlussCluster;

/// Check the desired topology before converging anything.
///
/// Returns `Some` blocked observation when `defaults.logReplicationFactor`
/// exceeds `tabletServers.replicas`; `None` otherwise (including when no
/// `defaults` section exists at all).
pub fn check(cluster: &FlussCluster) -> Option<Observation> {
    let rf = cluster.spec.defaults.as_ref()?.log_replication_factor;
    let tablets = cluster.spec.tablet_servers.replicas;
    if rf <= tablets {
        return None;
    }
    Some(Observation::TopologyBlocked {
        message: format!(
            "logReplicationFactor {rf} exceeds tabletServers replicas {tablets}; \
             add tablet servers or lower the replication factor"
        ),
    })
}
