// SPDX-License-Identifier: AGPL-3.0-only
//! Gateway API passthrough: SNI selects an owned TLS Service without decrypting traffic.

use crate::api::FlussCluster;
use crate::resources::{external_access, server_config::ConfigError};
use serde_json::{Value, json};

/// Desired Gateway and TLSRoutes for one cluster, suitable for GitOps or reconciliation.
pub fn manifests(cluster: &FlussCluster) -> Result<Value, ConfigError> {
    let endpoints = external_access::endpoints(cluster)?;
    let Some(external) = cluster
        .spec
        .listeners
        .as_ref()
        .and_then(|l| l.external.as_ref())
    else {
        return Ok(json!({"apiVersion": "v1", "kind": "List", "items": []}));
    };
    let cluster_name = cluster.metadata.name.as_deref().expect("named cluster");
    let name = format!("{cluster_name}-native");
    let namespace = cluster
        .metadata
        .namespace
        .as_deref()
        .expect("namespaced cluster");
    let mut items = vec![json!({
        "apiVersion": "gateway.networking.k8s.io/v1", "kind": "Gateway",
        "metadata": {"name": name, "namespace": namespace},
        "spec": {"gatewayClassName": external.gateway.class_name,
            "listeners": [{"name": "fluss-tls", "protocol": "TLS", "port": external.public_port,
                "tls": {"mode": "Passthrough"},
                "allowedRoutes": {"namespaces": {"from": "Same"}, "kinds": [{"kind": "TLSRoute"}]}}]}
    })];
    items.push(route(
        &name,
        namespace,
        "bootstrap",
        &external.domain,
        &format!("{cluster_name}-bootstrap"),
        external.public_port,
    ));
    for endpoint in endpoints {
        let (hostname, _) = endpoint
            .address
            .rsplit_once(':')
            .expect("validated hostname:port");
        items.push(route(
            &name,
            namespace,
            &endpoint.pod,
            hostname,
            &endpoint.service,
            external.public_port,
        ));
    }
    Ok(json!({"apiVersion": "v1", "kind": "List", "items": items}))
}

fn route(
    gateway: &str,
    namespace: &str,
    suffix: &str,
    hostname: &str,
    service: &str,
    port: i32,
) -> Value {
    json!({
        "apiVersion": "gateway.networking.k8s.io/v1", "kind": "TLSRoute",
        "metadata": {"name": format!("{gateway}-{suffix}"), "namespace": namespace},
        "spec": {"hostnames": [hostname],
            "parentRefs": [{"name": gateway, "sectionName": "fluss-tls"}],
            "rules": [{"backendRefs": [{"name": service, "port": port}]}]}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::external_access::tests::cluster;

    #[test]
    fn routes_each_hostname_to_exact_service_on_one_port() {
        let mut cluster = cluster();
        let before = manifests(&cluster).unwrap();
        let items = before["items"].as_array().unwrap();
        assert_eq!(items.len(), 6);
        assert_eq!(
            items[0]["spec"]["listeners"][0]["tls"]["mode"],
            "Passthrough"
        );
        assert_eq!(items[1]["spec"]["hostnames"][0], "fluss.example.com");
        assert_eq!(
            items[1]["spec"]["rules"][0]["backendRefs"][0]["name"],
            "spike-bootstrap"
        );
        assert_eq!(
            items[2]["spec"]["hostnames"][0],
            "coordinator-0.fluss.example.com"
        );
        for route in &items[1..] {
            assert_eq!(route["spec"]["rules"][0]["backendRefs"][0]["port"], 443);
        }
        cluster.spec.tablet_servers.replicas += 1;
        let after = manifests(&cluster).unwrap();
        assert_eq!(
            before["items"].as_array().unwrap(),
            &after["items"].as_array().unwrap()[..6]
        );
        assert_eq!(
            after["items"][6]["spec"]["hostnames"][0],
            "tablet-2.fluss.example.com"
        );
    }
}
