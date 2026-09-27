// SPDX-License-Identifier: AGPL-3.0-only
//! Render the JVM options `server.yaml` properties.
//!
//! The image reads `env.java.opts.<role>` from `server.yaml` (falling back
//! to `FLUSS_ENV_JAVA_OPTS*` env, which we never set). Heap from the spec
//! becomes `-Xms/-Xmx` with the same value — the standard run-it-pinned
//! posture — followed by any user `extraArgs` verbatim. Absent `jvm` means
//! absent keys: the image default applies, exactly like the Helm reference.

use std::collections::BTreeMap;

use crate::api::JvmSpec;

use super::ConfigError;

/// Key rendered here that users must not override: the heap comes from the
/// typed `jvm` section, not from free-form overrides.
pub(crate) const COORDINATOR_JVM_KEY: &str = "env.java.opts.coordinator-server";
pub(crate) const TABLET_JVM_KEY: &str = "env.java.opts.tablet-server";

/// Global JVM options have no typed equivalent (`extraArgs` covers the
/// per-role case) and their precedence against the role keys is unproven,
/// so they stay forbidden until proven.
pub(crate) const ENV_JAVA_OPTS_ALL: &str = "env.java.opts.all";

pub(crate) const PROTECTED_KEYS: &[&str] =
    &[COORDINATOR_JVM_KEY, TABLET_JVM_KEY, ENV_JAVA_OPTS_ALL];

/// Render the role's JVM options, or nothing when the CR sets no `jvm`.
pub(crate) fn properties(
    jvm: Option<&JvmSpec>,
    key: &str,
) -> Result<BTreeMap<String, String>, ConfigError> {
    match jvm {
        None => Ok(BTreeMap::new()),
        Some(jvm) => {
            let heap = normalize_heap(&jvm.heap, key)?;
            let mut opts = format!("-Xms{heap} -Xmx{heap}");
            for arg in &jvm.extra_args {
                opts.push(' ');
                opts.push_str(arg);
            }
            Ok(BTreeMap::from([(key.to_string(), opts)]))
        }
    }
}

/// Normalize a Kubernetes memory quantity to a JVM heap size.
///
/// The CR speaks Kubernetes (`512Mi`, `1Gi`); the JVM only understands
/// `K`/`M`/`G` suffixes, so `512Mi` would kill the server at boot with
/// `Invalid initial heap size`. Anything else is rejected fail-closed.
fn normalize_heap(heap: &str, key: &str) -> Result<String, ConfigError> {
    let upper = heap.to_uppercase();
    let trimmed = upper.strip_suffix('I').unwrap_or(&upper);
    let (digits, suffix) = trimmed.split_at(trimmed.len().saturating_sub(1));
    if !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && matches!(suffix.as_bytes(), [b'K' | b'M' | b'G'])
    {
        return Ok(trimmed.to_string());
    }
    Err(ConfigError::InvalidValue {
        key: key.to_string(),
        value: heap.to_string(),
        reason: "heap must be a Kubernetes quantity like 512Mi or 1Gi".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::normalize_heap;

    #[test]
    fn kubernetes_quantities_become_jvm_sizes() {
        for (input, expected) in [
            ("512Mi", "512M"),
            ("1Gi", "1G"),
            ("2G", "2G"),
            ("256m", "256M"),
            ("1ki", "1K"),
        ] {
            assert_eq!(
                normalize_heap(input, "env.java.opts.test").expect("valid heap"),
                expected,
                "input {input}"
            );
        }
    }

    #[test]
    fn bare_or_malformed_heaps_are_rejected() {
        for input in ["512", "", "abc", "1T", "Mi", "-5Mi"] {
            assert!(
                normalize_heap(input, "env.java.opts.test").is_err(),
                "input {input} must fail closed"
            );
        }
    }

    #[test]
    fn global_jvm_opts_override_is_forbidden() {
        use super::super::overrides;
        use std::collections::BTreeMap;
        let mut overrides_map = BTreeMap::new();
        overrides_map.insert(
            super::ENV_JAVA_OPTS_ALL.to_string(),
            "-Dfoo=bar".to_string(),
        );
        assert!(
            overrides::apply(BTreeMap::new(), &overrides_map, &BTreeMap::new()).is_err(),
            "unproven precedence must fail closed"
        );
    }
}
