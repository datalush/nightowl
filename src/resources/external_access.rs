// SPDX-License-Identifier: AGPL-3.0-only
//! One deterministic external identity for advertisements, Services and SNI routes.

use std::net::IpAddr;

use k8s_openapi::api::core::v1::Service;

use crate::api::{ExternalEndpointStatus, ExternalListenerSpec, FlussCluster};
use crate::constants::{
    COORDINATOR_STATEFULSET_SUFFIX, LABEL_ROLE, ROLE_TABLET, TABLET_STATEFULSET_SUFFIX,
};
use crate::resources::server_config::ConfigError;

pub fn validate(cluster: &FlussCluster) -> Result<(), ConfigError> {
    // Legacy CRs must not acquire an invented public identity or roll their pods.
    let Some(listeners) = cluster.spec.listeners.as_ref() else {
        return Ok(());
    };
    let Some(external) = listeners.external.as_ref() else {
        return Ok(());
    };
    let invalid = |reason: &str| ConfigError::InvalidValue {
        key: "listeners.external".into(),
        value: external.domain.clone(),
        reason: reason.into(),
    };
    if external.name.is_empty()
        || !external.name.starts_with(|c: char| c.is_ascii_alphabetic())
        || !external
            .name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        || external.name.eq_ignore_ascii_case(&listeners.internal.name)
        || external.name.eq_ignore_ascii_case(&listeners.client.name)
    {
        return Err(invalid(
            "listener name must be distinct and contain only ASCII letters, digits or underscores",
        ));
    }
    if !(1..=65535).contains(&external.port)
        || !(1..=65535).contains(&external.public_port)
        || external.port == listeners.internal.port
        || external.port == listeners.client.port
        || [
            external.port,
            listeners.internal.port,
            listeners.client.port,
        ]
        .contains(&super::tls_proxy::TLS_PORT)
    {
        return Err(invalid(
            "pod and public ports must be valid; pod port must differ from INTERNAL and CLIENT",
        ));
    }
    if external.domain.len() > 238
        || external.domain.parse::<IpAddr>().is_ok()
        || !external.domain.contains('.')
        || external.domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label.starts_with(|c: char| c.is_ascii_alphanumeric())
                || !label.ends_with(|c: char| c.is_ascii_alphanumeric())
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err(invalid(
            "domain must be a DNS suffix, not an address, scheme, port or shell expression",
        ));
    }
    if external.gateway.class_name.trim().is_empty()
        || external.tls.secret_name.trim().is_empty()
        || external
            .tls
            .image
            .as_ref()
            .is_some_and(|image| image.trim().is_empty())
    {
        return Err(invalid(
            "GatewayClass, TLS Secret and optional image must not be blank",
        ));
    }
    Ok(())
}

/// External hostname generated for one server ordinal. The same name is used for SNI and ACL transport.
pub fn hostname(external: &ExternalListenerSpec, role: &str, ordinal: i32) -> String {
    format!("{role}-{ordinal}.{}", external.domain)
}

/// Values appended to server.yaml at pod boot; only the ordinal varies.
pub fn advertised(external: &ExternalListenerSpec, role: &str) -> String {
    format!(
        "{}://{role}-$FLUSS_SERVER_ID.{}:{}",
        external.name, external.domain, external.public_port
    )
}

/// Include boot-time identity in controlled restart hashes, excluding replica counts.
pub(crate) fn rollout_input(cluster: &FlussCluster, _coordinator: bool, config: String) -> String {
    match cluster
        .spec
        .listeners
        .as_ref()
        .and_then(|l| l.external.as_ref())
    {
        Some(e) => format!(
            "{config}\nexternal={}:{}:{}:{}:{}:{}",
            e.name,
            e.port,
            e.domain,
            e.public_port,
            e.tls.secret_name,
            e.tls.image.as_deref().unwrap_or("default")
        ),
        None => config,
    }
}

pub fn endpoints(cluster: &FlussCluster) -> Result<Vec<ExternalEndpointStatus>, ConfigError> {
    validate(cluster)?;
    let Some(external) = cluster
        .spec
        .listeners
        .as_ref()
        .and_then(|l| l.external.as_ref())
    else {
        return Ok(vec![]);
    };
    let name = cluster
        .metadata
        .name
        .as_deref()
        .expect("reconciliation requires a name");
    let mut endpoints = Vec::new();
    for (role, suffix, replicas) in [
        (
            "coordinator",
            COORDINATOR_STATEFULSET_SUFFIX,
            cluster.spec.coordinator.replicas,
        ),
        (
            "tablet",
            TABLET_STATEFULSET_SUFFIX,
            cluster.spec.tablet_servers.replicas,
        ),
    ] {
        for ordinal in 0..replicas {
            let pod = format!("{name}{suffix}-{ordinal}");
            endpoints.push(ExternalEndpointStatus {
                service: format!("{pod}-external"),
                pod,
                address: format!(
                    "{}:{}",
                    hostname(external, role, ordinal),
                    external.public_port
                ),
                role: role.into(),
            });
        }
    }
    Ok(endpoints)
}

pub fn services(cluster: &FlussCluster) -> Result<Vec<Service>, ConfigError> {
    let endpoints = endpoints(cluster)?;
    let Some(external) = cluster
        .spec
        .listeners
        .as_ref()
        .and_then(|l| l.external.as_ref())
    else {
        return Ok(vec![]);
    };
    let mut services = Vec::new();
    for endpoint in endpoints {
        let mut service = super::client_service::desired_service(cluster);
        service.metadata.name = Some(endpoint.service);
        service.metadata.annotations = None;
        let spec = service.spec.as_mut().expect("client service has a spec");
        spec.selector
            .as_mut()
            .expect("client service has a selector")
            .insert("statefulset.kubernetes.io/pod-name".into(), endpoint.pod);
        let port = &mut spec.ports.as_mut().expect("client service has a port")[0];
        port.name = Some("tls".into());
        port.port = external.public_port;
        port.target_port = Some(
            k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(
                super::tls_proxy::TLS_PORT,
            ),
        );
        services.push(service);
    }
    let mut bootstrap = super::client_service::desired_service(cluster);
    bootstrap.metadata.name = Some(format!(
        "{}-bootstrap",
        cluster.metadata.name.as_deref().expect("named cluster")
    ));
    bootstrap.metadata.annotations = None;
    let spec = bootstrap.spec.as_mut().expect("client service has a spec");
    spec.selector
        .as_mut()
        .expect("client service has a selector")
        .insert(LABEL_ROLE.into(), ROLE_TABLET.into());
    let port = &mut spec.ports.as_mut().expect("client service has a port")[0];
    port.name = Some("tls".into());
    port.port = external.public_port;
    port.target_port = Some(
        k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(super::tls_proxy::TLS_PORT),
    );
    services.push(bootstrap);
    Ok(services)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::api::{ExternalDnsSpec, ExternalGatewaySpec, ExternalTlsSpec};

    pub(crate) fn cluster() -> FlussCluster {
        let mut cluster: FlussCluster =
            serde_yaml::from_str(include_str!("../../../lab2/demo.yml")).unwrap();
        cluster.metadata.uid = Some("test-uid".into());
        cluster.spec.coordinator.replicas = 2;
        cluster.spec.tablet_servers.replicas = 2;
        cluster.spec.listeners.as_mut().unwrap().external = Some(ExternalListenerSpec {
            name: "EXTERNAL".into(),
            port: 9125,
            public_port: 443,
            domain: "fluss.example.com".into(),
            gateway: ExternalGatewaySpec {
                class_name: "eg".into(),
            },
            tls: ExternalTlsSpec {
                secret_name: "fluss-tls".into(),
                image: None,
            },
            dns: ExternalDnsSpec::default(),
        });
        cluster
    }

    #[test]
    fn scale_keeps_names_and_pod_selection() {
        let mut cluster = cluster();
        let before = endpoints(&cluster).unwrap();
        let hash = crate::resources::statefulset::static_config_hash(&cluster, false).unwrap();
        cluster.spec.tablet_servers.replicas = 3;
        let after = endpoints(&cluster).unwrap();
        assert_eq!(before, after[..4]);
        assert_eq!(after[4].address, "tablet-2.fluss.example.com:443");
        assert_eq!(
            hash,
            crate::resources::statefulset::static_config_hash(&cluster, false).unwrap()
        );
        let services = services(&cluster).unwrap();
        assert_eq!(services.len(), 6);
        assert_eq!(
            services[5]
                .spec
                .as_ref()
                .unwrap()
                .selector
                .as_ref()
                .unwrap()[LABEL_ROLE],
            ROLE_TABLET
        );
        for (service, endpoint) in services.iter().zip(after) {
            let spec = service.spec.as_ref().unwrap();
            assert_eq!(
                spec.selector.as_ref().unwrap()["statefulset.kubernetes.io/pod-name"],
                endpoint.pod
            );
            assert_eq!(spec.ports.as_ref().unwrap()[0].port, 443);
        }
    }

    #[test]
    fn invalid_external_names_are_refused() {
        for domain in ["127.0.0.1", "bad..host", "$(id)", "localhost"] {
            let mut cluster = cluster();
            cluster
                .spec
                .listeners
                .as_mut()
                .unwrap()
                .external
                .as_mut()
                .unwrap()
                .domain = domain.into();
            assert!(validate(&cluster).is_err(), "{domain}");
        }
    }

    #[test]
    fn listener_advertises_public_names_but_binds_local() {
        let cluster = cluster();
        let external = cluster
            .spec
            .listeners
            .as_ref()
            .unwrap()
            .external
            .as_ref()
            .unwrap();
        assert_eq!(
            advertised(external, "tablet"),
            "EXTERNAL://tablet-$FLUSS_SERVER_ID.fluss.example.com:443"
        );
    }
}
