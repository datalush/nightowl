// SPDX-License-Identifier: AGPL-3.0-only
//! Ownership policy: never adopt objects owned by someone else.

/// True when the existing object is controlled by our FlussCluster.
///
/// Ownership is identified by uid, not by name: a deleted and recreated
/// FlussCluster keeps its name but gets a fresh uid, and must not inherit
/// the previous incarnation's resources silently.
pub(crate) fn owned_by<K: kube::Resource>(existing: &K, uid: &str) -> bool {
    existing
        .meta()
        .owner_references
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|owner| owner.uid == uid && owner.controller == Some(true))
}
