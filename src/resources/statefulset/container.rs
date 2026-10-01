// SPDX-License-Identifier: AGPL-3.0-only
//! The Fluss container: identity boot script, probes and resources.
//!
//! Each pod derives its own identity at boot (ordinal id plus pod-IP bind
//! and stable-DNS advertised listeners) through a small `sh` wrapper, then
//! execs the official server script. Tablets gate readiness on the image's
//! own cluster-health probe; the coordinator, which declares health rather
//! than asking for it, uses plain TCP.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EnvVar, EnvVarSource, ExecAction, ObjectFieldSelector, Probe,
    ResourceRequirements, TCPSocketAction,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

use super::{Build, Role, STAGING_DIR};
use crate::constants::{CONFIG_DATA_KEY, PORT_NAME_CLIENT, PORT_NAME_INTERNAL};

/// Official two-step readiness probe (local TCP + cluster-health GREEN
/// gate with anti-wedge latching), shipped in the image for StatefulSet
/// rolling upgrades.
const READINESS_SCRIPT: &str = "/opt/fluss/bin/readiness-check.sh";

impl<'a> Build<'a> {
    pub(super) fn container(&self) -> Container {
        Container {
            name: self.role.container_name().to_string(),
            image: Some(self.image.clone()),
            image_pull_policy: self.image_pull_policy(),
            command: Some(vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                self.boot_script(),
            ]),
            ports: Some(self.ports()),
            env: Some(self.env()),
            resources: Some(self.resources()),
            volume_mounts: Some(self.mounts()),
            readiness_probe: Some(self.readiness()),
            liveness_probe: Some(self.liveness()),
            ..Default::default()
        }
    }

    fn ports(&self) -> Vec<ContainerPort> {
        let listeners = self.listeners();
        let mut ports = vec![
            ContainerPort {
                name: Some(PORT_NAME_INTERNAL.to_string()),
                container_port: listeners.internal.port,
                protocol: Some("TCP".to_string()),
                ..Default::default()
            },
            ContainerPort {
                name: Some(PORT_NAME_CLIENT.to_string()),
                container_port: listeners.client.port,
                protocol: Some("TCP".to_string()),
                ..Default::default()
            },
        ];
        if let Some(external) = &listeners.external {
            ports.push(ContainerPort {
                name: Some("external".into()),
                container_port: external.port,
                protocol: Some("TCP".into()),
                ..Default::default()
            });
        }
        ports
    }

    /// Resource requirements straight from the component spec.
    ///
    /// No policy here: requests/limits are the user's, carried verbatim into
    /// Kubernetes quantities. JVM heap sizing stays out (see `server_config`).
    fn resources(&self) -> ResourceRequirements {
        let component = match self.role {
            Role::Coordinator => &self.cluster.spec.coordinator.resources,
            Role::Tablet => &self.cluster.spec.tablet_servers.resources,
        };
        let requests = BTreeMap::from([
            ("cpu".to_string(), Quantity(component.requests.cpu.clone())),
            (
                "memory".to_string(),
                Quantity(component.requests.memory.clone()),
            ),
        ]);
        let limits = component.limits.as_ref().map(|limits| {
            BTreeMap::from([
                ("cpu".to_string(), Quantity(limits.cpu.clone())),
                ("memory".to_string(), Quantity(limits.memory.clone())),
            ])
        });
        ResourceRequirements {
            requests: Some(requests),
            limits,
            ..Default::default()
        }
    }

    /// Boot script: stage the shared config, append the per-pod identity, and
    /// exec the official server script.
    ///
    /// The ConfigMap mount is read-only and the image rewrites its own
    /// `server.yaml` at boot, so the shared file is staged at [`STAGING_DIR`]
    /// and copied into the image `conf/` first — mounting it in place would
    /// break. The ordinal comes from the StatefulSet pod name (`<sts>-<n>`);
    /// bind uses the pod IP (no DNS round-trip at bind time) while clients are
    /// advertised the stable pod DNS. Shared keys stay in the staged file —
    /// only per-pod keys are appended here, mirroring the reserved-keys split
    /// in `server_config`.
    fn boot_script(&self) -> String {
        let listeners = self.listeners();
        // `$POD_*` expand at boot via the downward API; `{{` / `}}` are literal
        // braces for the shell, not format placeholders.
        let dns = format!(
            "$POD_NAME.{}.{}",
            self.service_name, "$POD_NAMESPACE.svc.cluster.local"
        );
        let mut bind = format!(
            "{}://$POD_IP:{}, {}://$POD_IP:{}",
            listeners.internal.name,
            listeners.internal.port,
            listeners.client.name,
            listeners.client.port,
        );
        let mut advertised = format!(
            "{}://{}:{}",
            listeners.client.name, dns, listeners.client.port
        );
        if let Some(external) = &listeners.external {
            // Only the local TLS sidecar can reach the plaintext EXTERNAL listener.
            bind.push_str(&format!(
                ", {}://127.0.0.1:{}",
                external.name, external.port
            ));
            let role = match self.role {
                Role::Coordinator => "coordinator",
                Role::Tablet => "tablet",
            };
            advertised.push_str(&format!(
                ", {}",
                crate::resources::external_access::advertised(external, role)
            ));
        }
        let id_line = match self.role {
            Role::Coordinator => String::new(),
            Role::Tablet => {
                "echo \"tablet-server.id: ${FLUSS_SERVER_ID}\" >> $FLUSS_HOME/conf/server.yaml && \\\n"
                    .to_string()
            }
        };
        format!(
            "export FLUSS_SERVER_ID=${{POD_NAME##*-}} && \\\ncp {STAGING_DIR}/{CONFIG_DATA_KEY} $FLUSS_HOME/conf && \\\n{id_line}echo \"bind.listeners: {bind}\" >> $FLUSS_HOME/conf/server.yaml && \\\necho \"advertised.listeners: {advertised}\" >> $FLUSS_HOME/conf/server.yaml && \\\nexec bin/{} start-foreground",
            self.role.script_name(),
        )
    }

    /// Container environment: pod identity for the boot script plus the health
    /// probe's dial target.
    fn env(&self) -> Vec<EnvVar> {
        let mut env = vec![
            field_ref_env("POD_NAME", "metadata.name"),
            field_ref_env("POD_NAMESPACE", "metadata.namespace"),
            field_ref_env("POD_IP", "status.podIP"),
        ];
        if self.role == Role::Tablet {
            let readiness_port = if self.cluster.spec.security.is_some() {
                self.listeners().internal.port
            } else {
                self.listeners().client.port
            };
            env.push(EnvVar {
                name: "READINESS_TCP_PORT".to_string(),
                value: Some(readiness_port.to_string()),
                ..Default::default()
            });
        }
        env
    }

    /// Tablet readiness: the official image probe (local TCP + cluster-health
    /// GREEN gate with anti-wedge latching). A Ready tablet means its share of
    /// the cluster recovered, not just an open port. Generous thresholds: the
    /// probe forks a JVM per run, and boot takes minutes.
    fn readiness(&self) -> Probe {
        match self.role {
            Role::Tablet => Probe {
                exec: Some(ExecAction {
                    command: Some(vec![READINESS_SCRIPT.to_string()]),
                }),
                initial_delay_seconds: Some(15),
                period_seconds: Some(10),
                timeout_seconds: Some(10),
                success_threshold: Some(1),
                failure_threshold: Some(100),
                ..Default::default()
            },
            // The Coordinator declares GREEN; with no local tablet to ask, an
            // open client port is the honest signal.
            Role::Coordinator => tcp_probe(self.listeners().client.port, 10, 3, 30),
        }
    }

    /// Liveness stays on TCP and tolerant: killing a JVM in a long GC pause
    /// looks exactly like instability. The client port is the one outsiders
    /// actually dial.
    fn liveness(&self) -> Probe {
        tcp_probe(self.listeners().client.port, 10, 1, 100)
    }
}

fn field_ref_env(name: &str, field_path: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value_from: Some(EnvVarSource {
            field_ref: Some(ObjectFieldSelector {
                // Stated explicitly: the apiserver defaults it to v1, and
                // an absent field would read as drift on every trigger.
                api_version: Some("v1".to_string()),
                field_path: field_path.to_string(),
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn tcp_probe(
    port: i32,
    period_seconds: i32,
    timeout_seconds: i32,
    failure_threshold: i32,
) -> Probe {
    Probe {
        tcp_socket: Some(TCPSocketAction {
            port: IntOrString::Int(port),
            ..Default::default()
        }),
        initial_delay_seconds: Some(10),
        period_seconds: Some(period_seconds),
        timeout_seconds: Some(timeout_seconds),
        // Stated explicitly like the rest: the apiserver defaults it to 1.
        success_threshold: Some(1),
        failure_threshold: Some(failure_threshold),
        ..Default::default()
    }
}
