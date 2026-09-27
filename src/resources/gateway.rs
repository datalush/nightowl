// SPDX-License-Identifier: AGPL-3.0-only
//! Optional Gateway: stock upstream Deployment plus Service plus optional
//! Ingress, all sharing one base name. Opt-in only (`spec.gateway.enabled`);
//! absent or disabled means the reconciler deletes these instead of
//! converging them.
//!
//! The Gateway is configured purely by environment (upstream's preferred
//! container path): one bootstrap variable pointing at this cluster's
//! coordinator. No ConfigMap, no version matrix — the image tag is user
//! data defaulting to the cluster release mate.

use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, HTTPGetAction, PodSpec, PodTemplateSpec, Probe, Service, ServicePort,
    ServiceSpec,
};
use k8s_openapi::api::networking::v1::{
    HTTPIngressPath, HTTPIngressRuleValue, Ingress, IngressBackend, IngressRule,
    IngressServiceBackend, IngressSpec, IngressTLS,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

use crate::api::{FlussCluster, GatewaySpec};
use crate::constants::{
    API_VERSION, GATEWAY_METRICS_PORT, GATEWAY_REST_PORT, GATEWAY_SUFFIX, KIND_FLUSS_CLUSTER,
    LABEL_CLUSTER, LABEL_ROLE, ROLE_GATEWAY,
};

/// Bootstrap env key (upstream documented mapping for containers).
const BOOTSTRAP_ENV: &str = "FLUSS_GATEWAY__CLUSTER__DEFAULT__BOOTSTRAP__SERVERS";

/// Default Gateway image mate for a cluster release.
fn default_image(cluster: &FlussCluster) -> String {
    format!("apache/fluss-gateway:{}", cluster.spec.version)
}

/// Shared base name for the Deployment, Service and Ingress.
pub fn base_name(cluster: &FlussCluster) -> String {
    let (base, _, _) = names(cluster);
    base
}

/// Deployment object name.
pub fn deployment_name(cluster: &FlussCluster) -> String {
    base_name(cluster)
}

/// Service object name.
pub fn service_name(cluster: &FlussCluster) -> String {
    base_name(cluster)
}

/// Ingress object name.
pub fn ingress_name(cluster: &FlussCluster) -> String {
    base_name(cluster)
}

fn names(cluster: &FlussCluster) -> (String, Option<String>, BTreeMap<String, String>) {
    let name = cluster
        .metadata
        .name
        .clone()
        .expect("FlussCluster needs a name");
    let namespace = cluster.metadata.namespace.clone();
    let base = format!("{name}{GATEWAY_SUFFIX}");
    let labels = BTreeMap::from([
        (LABEL_CLUSTER.to_string(), name.clone()),
        (LABEL_ROLE.to_string(), ROLE_GATEWAY.to_string()),
    ]);
    (base, namespace, labels)
}

fn owner_reference(cluster: &FlussCluster) -> OwnerReference {
    OwnerReference {
        api_version: API_VERSION.to_string(),
        kind: KIND_FLUSS_CLUSTER.to_string(),
        name: cluster
            .metadata
            .name
            .clone()
            .expect("FlussCluster needs a name"),
        uid: cluster
            .metadata
            .uid
            .clone()
            .expect("FlussCluster needs a uid"),
        controller: Some(true),
        block_owner_deletion: Some(true),
    }
}

fn gateway_spec(cluster: &FlussCluster) -> &GatewaySpec {
    cluster
        .spec
        .gateway
        .as_ref()
        .expect("gateway builder needs spec.gateway")
}

/// Coordinator-zero DNS over the internal listener: the Gateway is a native
/// client and bootstraps like one.
fn bootstrap_servers(cluster: &FlussCluster) -> String {
    let name = cluster
        .metadata
        .name
        .clone()
        .expect("FlussCluster needs a name");
    let namespace = cluster
        .metadata
        .namespace
        .clone()
        .expect("FlussCluster needs a namespace");
    let port = cluster
        .spec
        .listeners
        .as_ref()
        .expect("listeners is required for the gateway bootstrap")
        .internal
        .port;
    format!("{name}-coordinator-0.{name}-coordinator-headless.{namespace}.svc.cluster.local:{port}")
}

/// Desired Gateway Deployment: stock image, one env var, upstream probes.
pub fn desired_deployment(cluster: &FlussCluster) -> Deployment {
    let spec = gateway_spec(cluster);
    let (base, namespace, labels) = names(cluster);
    let image = spec.image.clone().unwrap_or_else(|| default_image(cluster));
    let replicas = spec.replicas;

    let container = Container {
        name: ROLE_GATEWAY.to_string(),
        image: Some(image),
        ports: Some(vec![
            ContainerPort {
                name: Some("rest".to_string()),
                container_port: GATEWAY_REST_PORT,
                ..Default::default()
            },
            ContainerPort {
                name: Some("metrics".to_string()),
                container_port: GATEWAY_METRICS_PORT,
                ..Default::default()
            },
        ]),
        env: Some(vec![k8s_openapi::api::core::v1::EnvVar {
            name: BOOTSTRAP_ENV.to_string(),
            value: Some(bootstrap_servers(cluster)),
            ..Default::default()
        }]),
        readiness_probe: Some(Probe {
            http_get: Some(HTTPGetAction {
                path: Some("/ready".to_string()),
                port: IntOrString::Int(GATEWAY_REST_PORT),
                ..Default::default()
            }),
            initial_delay_seconds: Some(5),
            period_seconds: Some(5),
            ..Default::default()
        }),
        liveness_probe: Some(Probe {
            http_get: Some(HTTPGetAction {
                path: Some("/health".to_string()),
                port: IntOrString::Int(GATEWAY_REST_PORT),
                ..Default::default()
            }),
            initial_delay_seconds: Some(10),
            period_seconds: Some(10),
            ..Default::default()
        }),
        ..Default::default()
    };

    Deployment {
        metadata: ObjectMeta {
            name: Some(base.clone()),
            namespace,
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner_reference(cluster)]),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(replicas),
            selector: LabelSelector {
                match_labels: Some(labels.clone()),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![container],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        status: None,
    }
}

/// Desired ClusterIP Service: REST plus metrics (the latter for future
/// scraping; harmless until then).
pub fn desired_service(cluster: &FlussCluster) -> Service {
    let (base, namespace, labels) = names(cluster);
    Service {
        metadata: ObjectMeta {
            name: Some(base),
            namespace,
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner_reference(cluster)]),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(labels),
            ports: Some(vec![
                ServicePort {
                    name: Some("rest".to_string()),
                    port: GATEWAY_REST_PORT,
                    protocol: Some("TCP".to_string()),
                    ..Default::default()
                },
                ServicePort {
                    name: Some("metrics".to_string()),
                    port: GATEWAY_METRICS_PORT,
                    protocol: Some("TCP".to_string()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        status: None,
    }
}

/// Desired Ingress: single host rule to the Service, optional class and
/// TLS reference. Both are environment data passed through verbatim — the
/// secret's existence is checked at reconcile time, the class is not.
pub fn desired_ingress(cluster: &FlussCluster) -> Ingress {
    let ingress = gateway_spec(cluster)
        .ingress
        .as_ref()
        .expect("gateway builder needs spec.gateway.ingress");
    let (base, namespace, labels) = names(cluster);
    let backend = IngressBackend {
        service: Some(IngressServiceBackend {
            name: base.clone(),
            port: Some(k8s_openapi::api::networking::v1::ServiceBackendPort {
                number: Some(GATEWAY_REST_PORT),
                ..Default::default()
            }),
        }),
        ..Default::default()
    };
    Ingress {
        metadata: ObjectMeta {
            name: Some(base),
            namespace,
            labels: Some(labels),
            annotations: ingress.class_name.as_ref().map(|class| {
                BTreeMap::from([("kubernetes.io/ingress.class".to_string(), class.clone())])
            }),
            owner_references: Some(vec![owner_reference(cluster)]),
            ..Default::default()
        },
        spec: Some(IngressSpec {
            default_backend: None,
            ingress_class_name: ingress.class_name.clone(),
            tls: ingress.tls_secret_name.as_ref().map(|secret| {
                vec![IngressTLS {
                    hosts: Some(vec![ingress.host.clone()]),
                    secret_name: Some(secret.clone()),
                }]
            }),
            rules: Some(vec![IngressRule {
                host: Some(ingress.host.clone()),
                http: Some(HTTPIngressRuleValue {
                    paths: vec![HTTPIngressPath {
                        path: Some("/".to_string()),
                        path_type: "Prefix".to_string(),
                        backend: backend.clone(),
                    }],
                }),
            }]),
        }),
        status: None,
    }
}

/// Deployment agrees on what the controller sets: replicas, image, env and
/// probes. Server-defaulted strategy/revision fields never read as drift.
pub fn same_deployment(a: &Deployment, b: &Deployment) -> bool {
    let (Some(a_spec), Some(b_spec)) = (a.spec.as_ref(), b.spec.as_ref()) else {
        return a.spec.is_none() && b.spec.is_none();
    };
    if a_spec.replicas != b_spec.replicas {
        return false;
    }
    let containers = |spec: &DeploymentSpec| {
        spec.template
            .spec
            .as_ref()
            .map(|pod| pod.containers.clone())
    };
    match (containers(a_spec), containers(b_spec)) {
        (Some(a_containers), Some(b_containers)) => {
            a_containers.len() == b_containers.len()
                && a_containers.iter().zip(b_containers.iter()).all(|(a, b)| {
                    a.image == b.image
                        && a.env == b.env
                        && a.ports == b.ports
                        && probe_key(&a.readiness_probe) == probe_key(&b.readiness_probe)
                        && probe_key(&a.liveness_probe) == probe_key(&b.liveness_probe)
                })
        }
        _ => false,
    }
}

fn probe_key(probe: &Option<Probe>) -> Option<(String, i32)> {
    probe.as_ref().and_then(|probe| {
        probe.http_get.as_ref().map(|get| {
            (
                get.path.clone().unwrap_or_default(),
                match &get.port {
                    IntOrString::Int(port) => port,
                    _ => &0,
                }
                .to_owned(),
            )
        })
    })
}

/// Service agrees on selector plus managed ports; clusterIP belongs to
/// the server.
pub fn same_service(a: &Service, b: &Service) -> bool {
    let (Some(a_spec), Some(b_spec)) = (a.spec.as_ref(), b.spec.as_ref()) else {
        return a.spec.is_none() && b.spec.is_none();
    };
    a_spec.selector == b_spec.selector
        && port_key(a_spec.ports.as_deref()) == port_key(b_spec.ports.as_deref())
}

fn port_key(ports: Option<&[ServicePort]>) -> Vec<(Option<String>, i32)> {
    ports
        .unwrap_or_default()
        .iter()
        .map(|port| (port.name.clone(), port.port))
        .collect()
}

/// Ingress agrees on rules, TLS references and class annotation; status
/// and controller-written fields belong to the environment.
pub fn same_ingress(a: &Ingress, b: &Ingress) -> bool {
    let (Some(a_spec), Some(b_spec)) = (a.spec.as_ref(), b.spec.as_ref()) else {
        return a.spec.is_none() && b.spec.is_none();
    };
    a_spec.rules == b_spec.rules
        && a_spec.tls == b_spec.tls
        && a_spec.ingress_class_name == b_spec.ingress_class_name
        && a.metadata.annotations == b.metadata.annotations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::GatewayIngressSpec;

    fn gateway_cluster() -> FlussCluster {
        let mut cluster: FlussCluster =
            serde_yaml::from_str(include_str!("../../../lab2/demo.yml"))
                .expect("lab2 demo CR must deserialize");
        cluster.metadata.uid = Some("test-uid".to_string());
        cluster.spec.gateway = Some(GatewaySpec {
            enabled: true,
            replicas: 2,
            image: None,
            ingress: Some(GatewayIngressSpec {
                host: "demo.fluss.datalush.com".to_string(),
                class_name: Some("traefik".to_string()),
                tls_secret_name: Some("demo-tls".to_string()),
            }),
        });
        cluster
    }

    #[test]
    fn deployment_wires_image_env_and_probes() {
        let cluster = gateway_cluster();
        let deployment = desired_deployment(&cluster);
        let container = deployment
            .spec
            .as_ref()
            .expect("deployment needs a spec")
            .template
            .spec
            .as_ref()
            .expect("pod template needs a spec")
            .containers
            .first()
            .expect("one gateway container");
        assert_eq!(
            container.image.as_deref(),
            Some("apache/fluss-gateway:1.0.0"),
            "image defaults to the cluster release mate, got: {:?}",
            container.image
        );
        let env: Vec<(String, String)> = container
            .env
            .as_ref()
            .expect("bootstrap env needed")
            .iter()
            .map(|var| {
                (
                    var.name.clone(),
                    var.value.clone().expect("literal env value"),
                )
            })
            .collect();
        assert!(
            env.iter().any(|(name, value)| name == BOOTSTRAP_ENV
                && value.contains("spike-coordinator-0")
                && value.ends_with(":9123")),
            "bootstrap points at the coordinator, got: {env:?}"
        );
        assert!(
            container.readiness_probe.is_some() && container.liveness_probe.is_some(),
            "upstream /ready and /health probes"
        );
    }

    #[test]
    fn ingress_carries_host_class_and_tls() {
        let cluster = gateway_cluster();
        let ingress = desired_ingress(&cluster);
        let spec = ingress.spec.expect("ingress needs a spec");
        let hosts: Vec<String> = spec
            .rules
            .expect("one host rule")
            .into_iter()
            .filter_map(|rule| rule.host)
            .collect();
        assert_eq!(hosts, vec!["demo.fluss.datalush.com".to_string()]);
        assert_eq!(spec.ingress_class_name.as_deref(), Some("traefik"));
        let tls = spec.tls.expect("tls reference needed");
        assert_eq!(tls[0].secret_name.as_deref(), Some("demo-tls"));
    }

    #[test]
    fn comparators_ignore_server_defaults() {
        let cluster = gateway_cluster();
        let mut live = desired_deployment(&cluster);
        live.spec.as_mut().expect("spec").strategy = None;
        assert!(
            same_deployment(&live, &desired_deployment(&cluster)),
            "server strategy defaults must not read as drift"
        );
    }
}
