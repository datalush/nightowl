// SPDX-License-Identifier: AGPL-3.0-only
//! A TLS-only sidecar terminates SNI traffic inside the server pod.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::ConfigMap;
use serde_json::json;

use crate::api::{ExternalListenerSpec, FlussCluster};

pub const TLS_PORT: i32 = 8443;
pub const CONFIG_VOLUME: &str = "native-tls-proxy-config";
pub const CERT_VOLUME: &str = "native-tls-cert";
pub const CONFIG_FILE: &str = "/etc/fluss-proxy/envoy.json";
pub const CERT_DIR: &str = "/etc/fluss-tls";
pub const DEFAULT_IMAGE: &str = "envoyproxy/envoy:v1.33.4";

/// Envoy accepts TLS at 8443; Fluss only accepts plaintext on pod loopback at EXTERNAL.port.
pub fn config(external: &ExternalListenerSpec) -> String {
    serde_json::to_string_pretty(&json!({
        "node": {"id": "fluss-native-sidecar", "cluster": "fluss-native"},
        "static_resources": {
            "listeners": [{
                "name": "fluss-tls",
                "address": {"socket_address": {"address": "0.0.0.0", "port_value": TLS_PORT}},
                "filter_chains": [{
                    "transport_socket": {
                        "name": "envoy.transport_sockets.tls",
                        "typed_config": {
                            "@type": "type.googleapis.com/envoy.extensions.transport_sockets.tls.v3.DownstreamTlsContext",
                            "common_tls_context": {"tls_certificate_sds_secret_configs": [{
                                "name": "fluss-tls",
                                "sds_config": {"path_config_source": {
                                    "path": "/etc/fluss-proxy/tls-secret.json"
                                }}
                            }]}
                        }
                    },
                    "filters": [{
                        "name": "envoy.filters.network.tcp_proxy",
                        "typed_config": {
                            "@type": "type.googleapis.com/envoy.extensions.filters.network.tcp_proxy.v3.TcpProxy",
                            "stat_prefix": "fluss_native", "cluster": "fluss-local"
                        }
                    }]
                }]
            }],
            "clusters": [{
                "name": "fluss-local", "type": "STATIC", "connect_timeout": "2s",
                "load_assignment": {"cluster_name": "fluss-local", "endpoints": [{
                    "lb_endpoints": [{"endpoint": {"address": {"socket_address": {
                        "address": "127.0.0.1", "port_value": external.port
                    }}}}]
                }]}
            }]
        }
    })).expect("static Envoy configuration is JSON-serializable")
}

fn tls_secret_config() -> String {
    serde_json::to_string_pretty(&json!({"resources": [{
        "@type": "type.googleapis.com/envoy.extensions.transport_sockets.tls.v3.Secret",
        "name": "fluss-tls",
        "tls_certificate": {
            "certificate_chain": {"filename": format!("{CERT_DIR}/tls.crt")},
            "private_key": {"filename": format!("{CERT_DIR}/tls.key")},
            "watched_directory": {"path": CERT_DIR}
        }
    }]}))
    .expect("static SDS resource is serializable")
}

/// Reuse the FlussCluster owner reference and labels of an existing operator ConfigMap.
pub fn desired_config_map(cluster: &FlussCluster) -> Option<ConfigMap> {
    let external = cluster.spec.listeners.as_ref()?.external.as_ref()?;
    let mut cm = super::config_map::desired_coordinator_config(cluster).ok()?;
    cm.metadata.name = cluster
        .metadata
        .name
        .as_deref()
        .map(|name| format!("{name}-native-tls"));
    cm.metadata.annotations = None;
    cm.data = Some(BTreeMap::from([
        ("envoy.json".into(), config(external)),
        ("tls-secret.json".into(), tls_secret_config()),
    ]));
    Some(cm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_sidecar_never_forwards_to_pod_ip_or_internal_listener() {
        let cluster = crate::resources::external_access::tests::cluster();
        let cm = desired_config_map(&cluster).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&cm.data.unwrap()["envoy.json"]).unwrap();
        assert_eq!(
            config["static_resources"]["clusters"][0]["load_assignment"]["endpoints"][0]["lb_endpoints"]
                [0]["endpoint"]["address"]["socket_address"]["address"],
            "127.0.0.1"
        );
        assert_eq!(
            config["static_resources"]["listeners"][0]["filter_chains"][0]["transport_socket"]["typed_config"]
                ["common_tls_context"]["tls_certificate_sds_secret_configs"][0]["sds_config"]["path_config_source"]
                ["path"],
            "/etc/fluss-proxy/tls-secret.json"
        );
        assert_eq!(
            config["static_resources"]["clusters"][0]["load_assignment"]["endpoints"][0]["lb_endpoints"]
                [0]["endpoint"]["address"]["socket_address"]["port_value"],
            9125
        );
    }
}
