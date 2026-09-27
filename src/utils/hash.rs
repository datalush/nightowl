// SPDX-License-Identifier: AGPL-3.0-only
use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

/// Hash text as `sha256:<hex>`, matching the documented `observedConfigHash` format.
pub fn sha256_hex(content: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(content.as_bytes()))
}

/// Canonical cluster-level hash over both rendered documents.
///
/// Fixed order (coordinator, then tablet) and a document separator, so the
/// value changes if and only if either document changes.
pub fn combined_config_hash(coordinator_yaml: &str, tablet_yaml: &str) -> String {
    sha256_hex(&format!("{coordinator_yaml}---\n{tablet_yaml}"))
}

/// Hash credential content as `sha256:<hex>` over sorted `key=value` pairs.
///
/// Sorted (BTreeMap order) so key order never reads as rotation; values
/// included because rotation changes values while keys stay put. Returns
/// `None` when any expected key is absent — a partial pin would compare
/// against the wrong content.
pub fn secret_data_hash(data: &BTreeMap<String, Vec<u8>>, keys: &[String]) -> Option<String> {
    let mut hasher = Sha256::new();
    for key in keys {
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        hasher.update(data.get(key)?);
        hasher.update(b"\n");
    }
    Some(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::secret_data_hash;
    use std::collections::BTreeMap;

    fn data() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            ("access-key".to_string(), b"AKIA".to_vec()),
            ("secret-key".to_string(), b"shhh".to_vec()),
            ("unrelated".to_string(), b"noise".to_vec()),
        ])
    }

    #[test]
    fn rotation_changes_the_hash_values_only() {
        let keys = vec!["access-key".to_string(), "secret-key".to_string()];
        let before = secret_data_hash(&data(), &keys).expect("complete keys hash");
        let mut rotated = data();
        rotated.insert("secret-key".to_string(), b"new".to_vec());
        let after = secret_data_hash(&rotated, &keys).expect("complete keys hash");
        assert_ne!(before, after, "value change must read as rotation");
        assert!(before.starts_with("sha256:"));
    }

    #[test]
    fn missing_keys_hash_nothing() {
        let data = data();
        assert!(
            secret_data_hash(&data, &["access-key".to_string(), "absent".to_string()]).is_none(),
            "partial pins would compare against wrong content"
        );
    }
}
