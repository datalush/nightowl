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
