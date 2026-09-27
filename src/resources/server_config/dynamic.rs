// SPDX-License-Identifier: AGPL-3.0-only
//! Dynamic-vs-restart config classification (j5v3 slice).
//!
//! Fluss applies a fixed allowlist of keys live via `Admin.alterClusterConfigs`
//! (mirrored from `DynamicServerConfig.ALLOWED_CONFIG_KEYS` in Fluss 1.0.0,
//! verified identical to main; the server's rejection stays authoritative —
//! on drift the operator reports instead of rolling). Everything else rides
//! the normal restart path through the pod-template hash.
//!
//! Two fail-closed rules shape the planner:
//! - Cluster-wide apply only: a dynamic key whose effective value differs
//!   between coordinator and tablets is NOT auto-applied (an Admin cluster
//!   call cannot express per-role values); it stays in the rollout hash.
//! - Status stores value *hashes*, never values: the allowlist holds
//!   credential-bearing keys (`security.sasl.plain.credentials`).

use std::collections::BTreeMap;

use crate::utils::hash;

/// Keys Fluss 1.0 applies live (see module docs for provenance).
const DYNAMIC_KEYS: &[&str] = &[
    "datalake.format",
    "log.retention.roll-active-segment.enabled",
    "log.replica.min-in-sync-replicas-number",
    "kv.leader-replica.memory-reserved",
    "kv.rocksdb.shared-rate-limiter.bytes-per-sec",
    "kv.snapshot.interval",
    "server.data-disk.write-recover-ratio",
    "server.data-disk.write-limit-ratio",
    "server.historical-partition.lookup-cache.max-disk-ratio",
    "server.historical-partition.lookuper-cache.expire-after-access",
    "server.historical-partition.thread-pool.max-size",
    "netty.server.max-queued-historical-requests",
    "remote.data.dirs",
    "remote.data.dirs.strategy",
    "remote.data.dirs.weights",
    "security.sasl.plain.credentials",
];

/// Key prefixes Fluss applies live.
const DYNAMIC_PREFIXES: &[&str] = &["datalake."];

/// True for allowlisted keys and prefixes; everything else (including
/// unknown future keys) takes the restart path.
pub(crate) fn is_dynamic_key(key: &str) -> bool {
    DYNAMIC_KEYS.contains(&key)
        || DYNAMIC_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

/// Dynamic subset appliable cluster-wide: present in both roles' merged
/// properties with identical values. Divergent values stay restart-bound
/// (see module docs).
pub(crate) fn appliable(
    coordinator: &BTreeMap<String, String>,
    tablets: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    coordinator
        .iter()
        .filter(|(key, _)| is_dynamic_key(key))
        .filter_map(|(key, value)| {
            tablets
                .get(key)
                .filter(|tablet_value| *tablet_value == value)
                .map(|_| (key.clone(), value.clone()))
        })
        .collect()
}

/// Merged properties minus the appliable dynamic subset: what the
/// pod-template rollout hash covers. Dynamic-only changes move no pods.
pub(crate) fn without_appliable(
    merged: &BTreeMap<String, String>,
    appliable: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    merged
        .iter()
        .filter(|(key, _)| !appliable.contains_key(*key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Planned Admin operations against the standing applied hashes:
/// set on new-or-changed values, delete on applied keys gone from desired.
pub(crate) fn plan(
    desired: &BTreeMap<String, String>,
    applied_hashes: &BTreeMap<String, String>,
) -> (Vec<(String, String)>, Vec<String>) {
    let mut set = Vec::new();
    for (key, value) in desired {
        let changed = applied_hashes
            .get(key)
            .is_none_or(|hash| *hash != hash::sha256_hex(value));
        if changed {
            set.push((key.clone(), value.clone()));
        }
    }
    let delete = applied_hashes
        .keys()
        .filter(|key| !desired.contains_key(*key))
        .cloned()
        .collect();
    (set, delete)
}

/// Standing applied hashes after a successful apply: sets recorded,
/// deletes forgotten.
pub(crate) fn applied_after(
    standing: &BTreeMap<String, String>,
    set: &[(String, String)],
    delete: &[String],
) -> BTreeMap<String, String> {
    let mut applied = standing.clone();
    for (key, value) in set {
        applied.insert(key.clone(), hash::sha256_hex(value));
    }
    for key in delete {
        applied.remove(key);
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maps(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn allowlist_covers_documented_keys_and_prefix() {
        assert!(is_dynamic_key("kv.snapshot.interval"));
        assert!(is_dynamic_key("datalake.format"));
        assert!(is_dynamic_key("datalake.anything.future"));
        assert!(!is_dynamic_key("bind.listeners"));
        assert!(!is_dynamic_key("some.unknown.future.key"));
    }

    #[test]
    fn divergent_role_values_stay_restart_bound() {
        let coordinator = maps(&[("kv.snapshot.interval", "10min")]);
        let tablets = maps(&[("kv.snapshot.interval", "30s")]);
        assert!(appliable(&coordinator, &tablets).is_empty());
        let tablets = maps(&[("kv.snapshot.interval", "10min")]);
        assert_eq!(
            appliable(&coordinator, &tablets),
            maps(&[("kv.snapshot.interval", "10min")])
        );
    }

    #[test]
    fn plan_sets_changed_and_deletes_gone() {
        let desired = maps(&[("kv.snapshot.interval", "30s")]);
        let applied = maps(&[("kv.snapshot.interval", &hash::sha256_hex("10min"))]);
        let (set, delete) = plan(&desired, &applied);
        assert_eq!(
            set,
            vec![("kv.snapshot.interval".to_string(), "30s".to_string())]
        );
        assert!(delete.is_empty());

        let (set, delete) = plan(&maps(&[]), &applied);
        assert!(set.is_empty());
        assert_eq!(delete, vec!["kv.snapshot.interval".to_string()]);
    }

    #[test]
    fn static_subset_drops_only_appliable_keys() {
        let merged = maps(&[
            ("kv.snapshot.interval", "30s"),
            ("bind.listeners", "x"),
            ("datalake.format", "paimon"),
        ]);
        let appliable = maps(&[("kv.snapshot.interval", "30s")]);
        let rest = without_appliable(&merged, &appliable);
        assert!(!rest.contains_key("kv.snapshot.interval"));
        assert!(rest.contains_key("bind.listeners"));
        // Present but not appliable (divergent roles): stays rollout-bound.
        assert!(rest.contains_key("datalake.format"));
    }
}
