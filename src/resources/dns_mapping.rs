// SPDX-License-Identifier: AGPL-3.0-only
//! Optional CoreDNS rewrite fragment; the platform, not the operator, installs it.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ConfigMap;

use crate::api::FlussCluster;
use crate::resources::{external_access, server_config::ConfigError};

pub fn name(cluster: &FlussCluster) -> String {
    format!(
        "{}-native-dns",
        cluster.metadata.name.as_deref().expect("named cluster")
    )
}

/// CoreDNS rewrite rules; exact names avoid shadowing another cluster's domain.
pub fn config(cluster: &FlussCluster) -> Result<Option<ConfigMap>, ConfigError> {
    let Some(external) = cluster
        .spec
        .listeners
        .as_ref()
        .and_then(|l| l.external.as_ref())
    else {
        return Ok(None);
    };
    if !external.dns.internal_mapping {
        return Ok(None);
    }
    let endpoints = external_access::endpoints(cluster)?;
    let namespace = cluster
        .metadata
        .namespace
        .as_deref()
        .expect("namespaced cluster");
    let mut rules = vec![format!(
        "rewrite name exact {} {}-bootstrap.{namespace}.svc.cluster.local",
        external.domain,
        cluster.metadata.name.as_deref().expect("named cluster")
    )];
    for endpoint in endpoints {
        let (hostname, _) = endpoint
            .address
            .rsplit_once(':')
            .expect("validated address");
        rules.push(format!(
            "rewrite name exact {hostname} {}.{namespace}.svc.cluster.local",
            endpoint.service,
        ));
    }
    let mut cm = super::client_service::desired_service(cluster);
    // A Service already has exactly the owner reference and cluster label we need.
    let metadata = std::mem::take(&mut cm.metadata);
    Ok(Some(ConfigMap {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(name(cluster)),
            annotations: None,
            ..metadata
        },
        data: Some(BTreeMap::from([(
            "rewrite.override".into(),
            format!("{}\n", rules.join("\n")),
        )])),
        ..Default::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_mapping_emits_nothing_and_enabled_maps_each_service() {
        let mut cluster = external_access::tests::cluster();
        assert!(config(&cluster).unwrap().is_none());
        cluster
            .spec
            .listeners
            .as_mut()
            .unwrap()
            .external
            .as_mut()
            .unwrap()
            .dns
            .internal_mapping = true;
        let data = config(&cluster).unwrap().unwrap().data.unwrap()["rewrite.override"].clone();
        assert!(data.contains(
            "rewrite name exact fluss.example.com spike-bootstrap.operator-dev.svc.cluster.local"
        ));
        assert!(data.contains("rewrite name exact tablet-0.fluss.example.com spike-tabletserver-0-external.operator-dev.svc.cluster.local"));
        assert_eq!(data.lines().count(), 5);
    }
}
