// SPDX-License-Identifier: AGPL-3.0-only
//! Prometheus scrape wiring: the reporter key in the base config, plus pod
//! annotations derived from the effective (merged) properties.
//!
//! The reporter is on unless the CR opts out (`observability.prometheus:
//! false`). The scrape annotations always describe the effective port, so a
//! user override of the port key keeps working without touching anything
//! else. Overriding the reporters key away from prometheus drops the
//! annotations: scraping something that is not there would be a lie.

use std::collections::BTreeMap;

use crate::api::FlussCluster;

pub(crate) const REPORTERS_KEY: &str = "metrics.reporters";
pub(crate) const PORT_KEY: &str = "metrics.reporter.prometheus.port";
pub(crate) const DEFAULT_PORT: &str = "9249";
pub(crate) const ANNOTATION_SCRAPE: &str = "prometheus.io/scrape";
pub(crate) const ANNOTATION_PORT: &str = "prometheus.io/port";

/// Effective flag: absent section means default-on.
pub(crate) fn prometheus_enabled(cluster: &FlussCluster) -> bool {
    cluster
        .spec
        .observability
        .as_ref()
        .map(|observability| observability.prometheus)
        .unwrap_or(true)
}

/// Base properties: the reporter key when enabled, nothing otherwise. User
/// overrides merge over this, so they keep precedence by construction.
pub(crate) fn base_properties(cluster: &FlussCluster) -> BTreeMap<String, String> {
    if prometheus_enabled(cluster) {
        BTreeMap::from([(REPORTERS_KEY.to_string(), "prometheus".to_string())])
    } else {
        BTreeMap::new()
    }
}

/// Effective scrape port from merged properties, or `None` when the
/// effective reporters value no longer includes prometheus.
pub(crate) fn scrape_port(properties: &BTreeMap<String, String>) -> Option<String> {
    properties
        .get(REPORTERS_KEY)
        .filter(|reporters| {
            reporters
                .split(',')
                .map(str::trim)
                .any(|reporter| reporter == "prometheus")
        })
        .map(|_| {
            properties
                .get(PORT_KEY)
                .cloned()
                .unwrap_or_else(|| DEFAULT_PORT.to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn default_port_when_reporter_present_without_port() {
        let merged = properties(&[(REPORTERS_KEY, "prometheus")]);
        assert_eq!(scrape_port(&merged).as_deref(), Some(DEFAULT_PORT));
    }

    #[test]
    fn custom_port_from_merged_properties() {
        let merged = properties(&[(REPORTERS_KEY, "prometheus"), (PORT_KEY, "9250")]);
        assert_eq!(scrape_port(&merged).as_deref(), Some("9250"));
    }

    #[test]
    fn no_annotations_when_reporters_overridden_away() {
        let merged = properties(&[(REPORTERS_KEY, "json")]);
        assert_eq!(scrape_port(&merged), None);
    }

    #[test]
    fn no_annotations_without_reporters_key() {
        assert_eq!(scrape_port(&properties(&[])), None);
    }
}
