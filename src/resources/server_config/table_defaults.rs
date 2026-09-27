// SPDX-License-Identifier: AGPL-3.0-only
use std::collections::BTreeMap;

use crate::api::{FlussCluster, TableDefaultsSpec};

use super::ConfigError;

/// server.yaml keys owned (read and written) by this module.
const REPLICATION_FACTOR_KEY: &str = "default.replication.factor";
const MIN_IN_SYNC_KEY: &str = "log.replica.min-in-sync-replicas-number";

/// Fluss server default when no replication factor is configured anywhere.
const FLUSS_DEFAULT_RF: i32 = 1;

/// Render the table-default `server.yaml` properties.
///
/// - `default.bucket.number` and `default.replication.factor` from the
///   cluster-wide `defaults` section when the CR sets it; absent entirely
///   otherwise, letting the Fluss server defaults apply.
///
/// Min-ISR is deliberately not here: [`ensure_quorum()`] owns it, because
/// the effective value depends on post-override state. Verified against
/// the Fluss 1.0 configuration reference and the absence of any per-table
/// min-ISR property in `TableConfig`: it is a server-level log
/// write-durability setting, not a per-table default.
/// No protected keys: every table default is soft by design, users may
/// refine any of them (including per role) through overrides.
pub(crate) const PROTECTED_KEYS: &[&str] = &[];

pub(crate) fn properties(cluster: &FlussCluster) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    if let Some(defaults) = cluster.spec.defaults.as_ref() {
        props.insert(
            "default.bucket.number".to_string(),
            defaults.table_buckets.to_string(),
        );
        props.insert(
            REPLICATION_FACTOR_KEY.to_string(),
            defaults.log_replication_factor.to_string(),
        );
    }
    props
}

/// Ensure the effective min-ISR in an already-merged property map.
///
/// - Min-ISR absent → insert quorum `floor(effective_RF / 2) + 1`.
/// - Min-ISR present → must parse as integer and satisfy `≤ effective_RF`.
/// - Neither key anywhere and no `defaults` section → nothing to do
///   (Fluss defaults rule).
///
/// Effective RF resolves merged map → structured section → Fluss default.
pub(crate) fn ensure_quorum(
    merged: &mut BTreeMap<String, String>,
    structured: Option<&TableDefaultsSpec>,
) -> Result<(), ConfigError> {
    if !merged.contains_key(REPLICATION_FACTOR_KEY)
        && !merged.contains_key(MIN_IN_SYNC_KEY)
        && structured.is_none()
    {
        return Ok(());
    }
    let effective_rf = match merged.get(REPLICATION_FACTOR_KEY) {
        Some(raw) => parse_positive(REPLICATION_FACTOR_KEY, raw)?,
        None => structured
            .map(|d| d.log_replication_factor)
            .unwrap_or(FLUSS_DEFAULT_RF),
    };
    match merged.get(MIN_IN_SYNC_KEY) {
        Some(raw) => {
            let value = parse_positive(MIN_IN_SYNC_KEY, raw)?;
            if value > effective_rf {
                return Err(ConfigError::InvalidValue {
                    key: MIN_IN_SYNC_KEY.to_string(),
                    value: raw.clone(),
                    reason: format!(
                        "must not exceed the effective replication factor ({effective_rf})"
                    ),
                });
            }
        }
        None => {
            merged.insert(
                MIN_IN_SYNC_KEY.to_string(),
                (effective_rf / 2 + 1).to_string(),
            );
        }
    }
    Ok(())
}

fn parse_positive(key: &str, raw: &str) -> Result<i32, ConfigError> {
    let value = raw.parse::<i32>().map_err(|_| ConfigError::InvalidValue {
        key: key.to_string(),
        value: raw.to_string(),
        reason: "must be an integer".to_string(),
    })?;
    if value < 1 {
        return Err(ConfigError::InvalidValue {
            key: key.to_string(),
            value: raw.to_string(),
            reason: "must be positive".to_string(),
        });
    }
    Ok(value)
}
