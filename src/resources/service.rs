//! Shared Service comparison: managed fields only.
//!
//! The apiserver defaults several Service fields on write (`targetPort`,
//! `internalTrafficPolicy`, allocated `clusterIP` on ClusterIP services).
//! Comparing whole specs reads every read-back as drift and rewrites the
//! object on every trigger — the benign extra update this replaces. Each
//! comparator below names exactly what the controller owns.

use k8s_openapi::api::core::v1::{Service, ServicePort};

/// Headless Services: fixed `clusterIP: None`, our selector, our ports.
pub fn same_headless_service(a: &Service, b: &Service) -> bool {
    let (Some(a_spec), Some(b_spec)) = (a.spec.as_ref(), b.spec.as_ref()) else {
        return a.spec.is_none() && b.spec.is_none();
    };
    a_spec.cluster_ip == b_spec.cluster_ip
        && a_spec.selector == b_spec.selector
        && same_ports(a_spec.ports.as_deref(), b_spec.ports.as_deref())
}

/// Client Service: the selector, the client port, and the user annotations
/// from the CR. `clusterIP` is server-allocated and never compared.
pub fn same_client_service(a: &Service, b: &Service) -> bool {
    let (Some(a_spec), Some(b_spec)) = (a.spec.as_ref(), b.spec.as_ref()) else {
        return a.spec.is_none() && b.spec.is_none();
    };
    a_spec.selector == b_spec.selector
        && same_ports(a_spec.ports.as_deref(), b_spec.ports.as_deref())
        && a.metadata.annotations == b.metadata.annotations
}

/// Ports agree on what the controller sets: name, port, protocol.
/// `targetPort` (defaulted to `port`) and `nodePort` belong to the server.
fn same_ports(a: Option<&[ServicePort]>, b: Option<&[ServicePort]>) -> bool {
    fn key(ports: &[ServicePort]) -> Vec<(Option<String>, i32, Option<String>)> {
        ports
            .iter()
            .map(|port| (port.name.clone(), port.port, port.protocol.clone()))
            .collect()
    }
    key(a.unwrap_or_default()) == key(b.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::{same_client_service, same_headless_service};
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

    fn spike_cluster() -> crate::api::FlussCluster {
        let mut cluster: crate::api::FlussCluster =
            serde_yaml::from_str(include_str!("../../../lab2/demo.yml"))
                .expect("lab2 demo CR must deserialize");
        cluster.metadata.uid = Some("test-uid".to_string());
        cluster
    }

    /// The apiserver fills defaults on write; a read-back with those
    /// defaults must still compare equal, or every trigger rewrites.
    fn server_defaulted(
        mut svc: k8s_openapi::api::core::v1::Service,
    ) -> k8s_openapi::api::core::v1::Service {
        let spec = svc.spec.as_mut().expect("service needs a spec");
        spec.internal_traffic_policy = Some("Cluster".to_string());
        for port in spec.ports.as_mut().expect("service needs ports") {
            port.target_port = Some(IntOrString::Int(port.port));
        }
        svc
    }

    #[test]
    fn headless_services_ignore_server_defaults() {
        let cluster = spike_cluster();
        let desired = super::super::coordinator_service::desired_service(&cluster);
        let live = server_defaulted(desired.clone());
        assert!(
            same_headless_service(&desired, &live),
            "targetPort and traffic policy must not read as drift"
        );
    }

    #[test]
    fn client_service_ignores_allocated_cluster_ip() {
        let cluster = spike_cluster();
        let desired = super::super::client_service::desired_service(&cluster);
        let mut live = server_defaulted(desired.clone());
        live.spec.as_mut().expect("service needs a spec").cluster_ip =
            Some("10.43.0.7".to_string());
        assert!(
            same_client_service(&desired, &live),
            "allocated clusterIP must not read as drift"
        );
    }

    #[test]
    fn real_changes_still_count() {
        let cluster = spike_cluster();
        let desired = super::super::client_service::desired_service(&cluster);
        let mut live = desired.clone();
        live.spec.as_mut().expect("service needs a spec").ports = Some(vec![]);
        assert!(
            !same_client_service(&desired, &live),
            "dropped ports must read as drift"
        );
    }
}
