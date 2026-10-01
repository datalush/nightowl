// SPDX-License-Identifier: AGPL-3.0-only
//! TLS terminator alongside Fluss: certificate stays in the target pod.

use k8s_openapi::api::core::v1::{
    Container, ContainerPort, Probe, ResourceRequirements, TCPSocketAction, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use std::collections::BTreeMap;

use super::Build;
use crate::resources::tls_proxy;

impl<'a> Build<'a> {
    pub(super) fn containers(&self) -> Vec<Container> {
        let mut containers = vec![self.container()];
        if let Some(proxy) = self.tls_container() {
            containers.push(proxy);
        }
        containers
    }

    fn tls_container(&self) -> Option<Container> {
        let external = self.cluster.spec.listeners.as_ref()?.external.as_ref()?;
        Some(Container {
            name: "native-tls-proxy".into(),
            image: Some(
                external
                    .tls
                    .image
                    .clone()
                    .unwrap_or_else(|| tls_proxy::DEFAULT_IMAGE.into()),
            ),
            command: Some(vec!["envoy".into()]),
            args: Some(vec![
                "-c".into(),
                tls_proxy::CONFIG_FILE.into(),
                "--log-level".into(),
                "warning".into(),
            ]),
            ports: Some(vec![ContainerPort {
                name: Some("native-tls".into()),
                container_port: tls_proxy::TLS_PORT,
                protocol: Some("TCP".into()),
                ..Default::default()
            }]),
            volume_mounts: Some(vec![
                VolumeMount {
                    name: tls_proxy::CONFIG_VOLUME.into(),
                    mount_path: "/etc/fluss-proxy".into(),
                    read_only: Some(true),
                    ..Default::default()
                },
                VolumeMount {
                    name: tls_proxy::CERT_VOLUME.into(),
                    mount_path: tls_proxy::CERT_DIR.into(),
                    read_only: Some(true),
                    ..Default::default()
                },
            ]),
            readiness_probe: Some(Probe {
                tcp_socket: Some(TCPSocketAction {
                    port: IntOrString::Int(tls_proxy::TLS_PORT),
                    ..Default::default()
                }),
                period_seconds: Some(5),
                timeout_seconds: Some(2),
                failure_threshold: Some(3),
                success_threshold: Some(1),
                ..Default::default()
            }),
            resources: Some(ResourceRequirements {
                requests: Some(BTreeMap::from([
                    ("cpu".into(), Quantity("50m".into())),
                    ("memory".into(), Quantity("64Mi".into())),
                ])),
                limits: Some(BTreeMap::from([
                    ("cpu".into(), Quantity("500m".into())),
                    ("memory".into(), Quantity("256Mi".into())),
                ])),
                ..Default::default()
            }),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{NativeSecuritySpec, SaslPlainSpec};
    use crate::resources::statefulset::desired_tablet_statefulset;

    #[test]
    fn tls_sidecar_and_local_plaintext_are_coherent() {
        let cluster = crate::resources::external_access::tests::cluster();
        let sts = desired_tablet_statefulset(&cluster, None).unwrap();
        let template = sts.spec.unwrap().template.spec.unwrap();
        assert_eq!(template.containers.len(), 2);
        let boot = template.containers[0].command.as_ref().unwrap()[2].as_str();
        assert!(boot.contains("EXTERNAL://127.0.0.1:9125"));
        assert!(boot.contains("EXTERNAL://tablet-$FLUSS_SERVER_ID.fluss.example.com:443"));
        let sidecar = &template.containers[1];
        assert_eq!(sidecar.name, "native-tls-proxy");
        assert_eq!(
            sidecar.ports.as_ref().unwrap()[0].container_port,
            tls_proxy::TLS_PORT
        );
        assert_eq!(sidecar.volume_mounts.as_ref().unwrap().len(), 2);
        let secret = template
            .volumes
            .unwrap()
            .into_iter()
            .find(|volume| volume.name == tls_proxy::CERT_VOLUME)
            .unwrap();
        assert_eq!(
            secret.secret.unwrap().secret_name.as_deref(),
            Some("fluss-tls")
        );
    }

    #[test]
    fn authenticated_clients_do_not_break_tablet_health_probe() {
        let mut cluster = crate::resources::external_access::tests::cluster();
        cluster.spec.security = Some(NativeSecuritySpec {
            sasl_plain: SaslPlainSpec {
                credentials_secret_name: "users".into(),
                admin_user: "admin".into(),
            },
        });
        let sts = desired_tablet_statefulset(&cluster, None).unwrap();
        let pod = sts.spec.unwrap().template.spec.unwrap();
        let fluss = &pod.containers[0];
        let probe_port = fluss
            .env
            .as_ref()
            .unwrap()
            .iter()
            .find(|env| env.name == "READINESS_TCP_PORT")
            .unwrap();
        assert_eq!(probe_port.value.as_deref(), Some("9123"));
        let secret = pod
            .volumes
            .unwrap()
            .into_iter()
            .find(|volume| volume.name == "sasl-credentials")
            .unwrap();
        assert_eq!(secret.secret.unwrap().secret_name.as_deref(), Some("users"));
    }
}
