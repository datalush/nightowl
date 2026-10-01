// SPDX-License-Identifier: AGPL-3.0-only
//! Limit plaintext INTERNAL and CLIENT access to Fluss pods and the operator.

use std::collections::BTreeMap;

use k8s_openapi::api::networking::v1::{
    NetworkPolicy, NetworkPolicyIngressRule, NetworkPolicyPeer, NetworkPolicyPort,
    NetworkPolicySpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

use crate::api::FlussCluster;
use crate::constants::{LABEL_CLUSTER, LABEL_ROLE, ROLE_COORDINATOR, ROLE_TABLET};
use crate::resources::tls_proxy;

pub fn desired_policy(cluster: &FlussCluster) -> Option<NetworkPolicy> {
    cluster.spec.listeners.as_ref()?.external.as_ref()?;
    let name = cluster.metadata.name.as_deref().expect("named cluster");
    let mut metadata = super::client_service::desired_service(cluster).metadata;
    metadata.name = Some(format!("{name}-native-ports"));
    metadata.annotations = None;
    let pod_selector = LabelSelector {
        match_labels: Some(BTreeMap::from([(LABEL_CLUSTER.into(), name.into())])),
        ..Default::default()
    };
    let server_peers = vec![
        NetworkPolicyPeer {
            pod_selector: Some(LabelSelector {
                match_labels: Some(BTreeMap::from([
                    (LABEL_CLUSTER.into(), name.into()),
                    (LABEL_ROLE.into(), ROLE_COORDINATOR.into()),
                ])),
                ..Default::default()
            }),
            ..Default::default()
        },
        NetworkPolicyPeer {
            pod_selector: Some(LabelSelector {
                match_labels: Some(BTreeMap::from([
                    (LABEL_CLUSTER.into(), name.into()),
                    (LABEL_ROLE.into(), ROLE_TABLET.into()),
                ])),
                ..Default::default()
            }),
            ..Default::default()
        },
        NetworkPolicyPeer {
            namespace_selector: Some(LabelSelector {
                match_labels: Some(BTreeMap::from([(
                    "kubernetes.io/metadata.name".into(),
                    "operator-system".into(),
                )])),
                ..Default::default()
            }),
            pod_selector: Some(LabelSelector {
                match_labels: Some(BTreeMap::from([(
                    "app.kubernetes.io/name".into(),
                    "nightowl".into(),
                )])),
                ..Default::default()
            }),
            ..Default::default()
        },
    ];
    let internal = cluster.spec.resolved_listeners();
    let port = |port| NetworkPolicyPort {
        port: Some(IntOrString::Int(port)),
        protocol: Some("TCP".into()),
        ..Default::default()
    };
    Some(NetworkPolicy {
        metadata: ObjectMeta { ..metadata },
        spec: Some(NetworkPolicySpec {
            pod_selector: Some(pod_selector),
            policy_types: Some(vec!["Ingress".into()]),
            ingress: Some(vec![
                NetworkPolicyIngressRule {
                    from: Some(server_peers),
                    ports: Some(vec![
                        port(internal.internal.port),
                        port(internal.client.port),
                    ]),
                },
                // TLS is authenticated at the sidecar, regardless of the client's namespace.
                NetworkPolicyIngressRule {
                    from: None,
                    ports: Some(vec![port(tls_proxy::TLS_PORT)]),
                },
            ]),
            ..Default::default()
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_ports_restricted_but_tls_exposed() {
        let cluster = crate::resources::external_access::tests::cluster();
        let policy = desired_policy(&cluster).unwrap();
        let ingress = policy.spec.unwrap().ingress.unwrap();
        assert_eq!(ingress[0].ports.as_ref().unwrap().len(), 2);
        assert_eq!(ingress[0].from.as_ref().unwrap().len(), 3);
        assert!(ingress[1].from.is_none());
        assert_eq!(
            ingress[1].ports.as_ref().unwrap()[0].port,
            Some(IntOrString::Int(8443))
        );
    }
}
