use std::collections::BTreeMap;

/// Render a property map as `key: value` lines, one per line.
///
/// Iteration follows `BTreeMap` key order, so the output is deterministic
/// for a given input map — a prerequisite for the future content hash.
pub fn to_yaml(properties: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (key, value) in properties {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(value);
        out.push('\n');
    }
    out
}
