//! Desired Coordinator / TabletServer StatefulSets.
//!
//! Mirrors `config_map.rs`: one file, both roles, pure functions of the CR.
//! Each pod stages the shared `server.yaml` ConfigMap into the image's
//! `conf/` at boot (the entrypoint rewrites that file, so a direct read-only
//! mount would break it), appends its own per-pod identity — ordinal id plus
//! pod-IP bind and stable-DNS advertised listeners — and execs the official
//! server script. The pod template carries the role's config hash, so any
//! rendered config change rolls the StatefulSet.
//!
//! The boot sequence and probe budgets mirror the proven Helm reference
//! running in the lab (`fluss` namespace): bind to pod IP, advertise stable
//! pod DNS, TCP liveness on the client port, and the image's own
//! `readiness-check.sh` (local TCP + cluster-health GREEN gate) for tablet
//! readiness.

use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, Container, ContainerPort, EmptyDirVolumeSource, EnvVar, EnvVarSource,
    ExecAction, KeyToPath, LocalObjectReference, ObjectFieldSelector, PodSpec, PodTemplateSpec,
    Probe, ResourceRequirements, SecretVolumeSource, TCPSocketAction, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

use crate::api::PodTemplateSpec as FlussPodTemplate;
use crate::api::{FlussCluster, ImagePullPolicy, ListenersSpec, S3AuthenticationSpec};
use crate::constants::{
    API_VERSION, CONFIG_DATA_KEY, CONFIG_HASH_ANNOTATION, COORDINATOR_CONFIG_SUFFIX,
    COORDINATOR_HEADLESS_SUFFIX, COORDINATOR_STATEFULSET_SUFFIX, KIND_FLUSS_CLUSTER, LABEL_CLUSTER,
    LABEL_ROLE, PORT_NAME_INTERNAL, ROLE_COORDINATOR, ROLE_TABLET, S3_ACCESS_KEY_FILE,
    S3_SECRET_KEY_FILE, S3_SECRETS_DIR, TABLET_CONFIG_SUFFIX, TABLET_HEADLESS_SUFFIX,
    TABLET_STATEFULSET_SUFFIX,
};
use crate::utils::hash;

use super::config_map;
use super::server_config::{self, ConfigError};

/// Staging path for the mounted ConfigMap: the image rewrites its own
/// `server.yaml` at boot, so the read-only ConfigMap mount must land
/// elsewhere and be copied over, never mounted in place.
const STAGING_DIR: &str = "/opt/operator-conf";

/// Official two-step readiness probe (local TCP + cluster-health GREEN
/// gate with anti-wedge latching), shipped in the image for StatefulSet
/// rolling upgrades.
const READINESS_SCRIPT: &str = "/opt/fluss/bin/readiness-check.sh";

/// Volume holding the staged `server.yaml`.
const CONFIG_VOLUME: &str = "server-config";

/// Volume holding the mounted S3 Secret for secret auth. Absent under
/// workload identity, where the credential chain needs no files.
const S3_CREDENTIALS_VOLUME: &str = "s3-credentials";

/// Placeholder data volume until 7vp7 renders `volumeClaimTemplates`.
const DATA_VOLUME: &str = "data";

/// Desired Coordinator StatefulSet, with its config hash pinned in the pod
/// template so rendered config changes roll the pods.
pub fn desired_coordinator_statefulset(cluster: &FlussCluster) -> Result<StatefulSet, ConfigError> {
    Build::assemble(cluster, Role::Coordinator).map(|build| build.build())
}

/// Desired TabletServer StatefulSet. See [`desired_coordinator_statefulset`].
pub fn desired_tablet_statefulset(cluster: &FlussCluster) -> Result<StatefulSet, ConfigError> {
    Build::assemble(cluster, Role::Tablet).map(|build| build.build())
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Coordinator,
    Tablet,
}

impl Role {
    fn suffix(self) -> &'static str {
        match self {
            Role::Coordinator => COORDINATOR_STATEFULSET_SUFFIX,
            Role::Tablet => TABLET_STATEFULSET_SUFFIX,
        }
    }

    fn config_map_suffix(self) -> &'static str {
        match self {
            Role::Coordinator => COORDINATOR_CONFIG_SUFFIX,
            Role::Tablet => TABLET_CONFIG_SUFFIX,
        }
    }

    fn container_name(self) -> &'static str {
        match self {
            Role::Coordinator => ROLE_COORDINATOR,
            Role::Tablet => ROLE_TABLET,
        }
    }

    fn label(self) -> &'static str {
        self.container_name()
    }

    fn headless_suffix(self) -> &'static str {
        match self {
            Role::Coordinator => COORDINATOR_HEADLESS_SUFFIX,
            Role::Tablet => TABLET_HEADLESS_SUFFIX,
        }
    }

    fn script_name(self) -> &'static str {
        match self {
            Role::Coordinator => "coordinator-server.sh",
            Role::Tablet => "tablet-server.sh",
        }
    }
}

/// Resolved inputs for one role's StatefulSet, assembled once and then
/// consumed method by method — so no function in this file takes more than
/// two arguments and the shape stays uniform across roles.
struct Build<'a> {
    cluster: &'a FlussCluster,
    role: Role,
    cluster_name: String,
    service_name: String,
    image: String,
    labels: BTreeMap<String, String>,
    config_hash: String,
}

impl<'a> Build<'a> {
    /// Resolve everything `build` needs. Rendering the `server.yaml` is the
    /// only fallible step, so it happens here, once.
    fn assemble(cluster: &'a FlussCluster, role: Role) -> Result<Self, ConfigError> {
        let cluster_name = cluster
            .metadata
            .name
            .clone()
            .expect("FlussCluster needs a name");
        let server_yaml = match role {
            Role::Coordinator => config_map::coordinator_server_yaml(cluster)?,
            Role::Tablet => config_map::tablet_server_yaml(cluster)?,
        };
        let fallback = format!("{}:{}", cluster.spec.image.repository, cluster.spec.version);
        let image = match role {
            Role::Coordinator => cluster.spec.coordinator.image.clone().unwrap_or(fallback),
            Role::Tablet => cluster
                .spec
                .tablet_servers
                .image
                .clone()
                .unwrap_or(fallback),
        };
        Ok(Self {
            cluster,
            role,
            service_name: format!("{cluster_name}{}", role.headless_suffix()),
            labels: BTreeMap::from([
                (LABEL_CLUSTER.to_string(), cluster_name.clone()),
                (LABEL_ROLE.to_string(), role.label().to_string()),
            ]),
            config_hash: hash::sha256_hex(&server_yaml),
            cluster_name,
            image,
        })
    }

    fn build(&self) -> StatefulSet {
        StatefulSet {
            metadata: self.object_meta(),
            spec: Some(StatefulSetSpec {
                replicas: Some(self.replicas()),
                selector: LabelSelector {
                    match_labels: Some(self.labels.clone()),
                    ..Default::default()
                },
                service_name: Some(self.service_name.clone()),
                template: self.pod_template(),
                ..Default::default()
            }),
            status: None,
        }
    }

    fn object_meta(&self) -> ObjectMeta {
        ObjectMeta {
            name: Some(format!("{}{}", self.cluster_name, self.role.suffix())),
            namespace: self.cluster.metadata.namespace.clone(),
            labels: Some(self.labels.clone()),
            owner_references: Some(vec![self.owner_reference()]),
            ..Default::default()
        }
    }

    fn container(&self) -> Container {
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

    fn pod_template(&self) -> PodTemplateSpec {
        // User pod labels/annotations ride along; the operator's own entries
        // win on collision, and the selector stays fixed so extra template
        // labels can never break pod matching.
        let overlay = self.pod_overlay();
        let mut template_labels = self.labels.clone();
        let mut template_annotations = BTreeMap::new();
        if let Some(t) = overlay {
            template_labels.extend(t.labels.clone());
            template_annotations.extend(t.annotations.clone());
        }
        template_annotations.insert(CONFIG_HASH_ANNOTATION.to_string(), self.config_hash.clone());

        PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: Some(template_labels),
                annotations: Some(template_annotations),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![self.container()],
                volumes: Some(self.volumes()),
                security_context: overlay.and_then(|t| t.security_context.clone()),
                image_pull_secrets: Some(
                    self.cluster
                        .spec
                        .image
                        .pull_secrets
                        .iter()
                        .map(|name| LocalObjectReference { name: name.clone() })
                        .collect(),
                ),
                ..Default::default()
            }),
        }
    }

    fn replicas(&self) -> i32 {
        match self.role {
            Role::Coordinator => self.cluster.spec.coordinator.replicas,
            Role::Tablet => self.cluster.spec.tablet_servers.replicas,
        }
    }

    fn listeners(&self) -> &ListenersSpec {
        self.cluster
            .spec
            .listeners
            .as_ref()
            .expect("listeners is required for the StatefulSet pods")
    }

    fn pod_overlay(&self) -> Option<&FlussPodTemplate> {
        match self.role {
            Role::Coordinator => self.cluster.spec.coordinator.pod_template.as_ref(),
            Role::Tablet => self.cluster.spec.tablet_servers.pod_template.as_ref(),
        }
    }

    fn owner_reference(&self) -> OwnerReference {
        let uid = self
            .cluster
            .metadata
            .uid
            .clone()
            .expect("FlussCluster needs a uid");
        OwnerReference {
            api_version: API_VERSION.to_string(),
            kind: KIND_FLUSS_CLUSTER.to_string(),
            name: self.cluster_name.clone(),
            uid,
            controller: Some(true),
            block_owner_deletion: Some(true),
        }
    }

    /// Pull policy straight from the spec; absent means the Kubernetes default.
    fn image_pull_policy(&self) -> Option<String> {
        self.cluster
            .spec
            .image
            .pull_policy
            .as_ref()
            .map(|policy| match policy {
                ImagePullPolicy::Always => "Always".to_string(),
                ImagePullPolicy::IfNotPresent => "IfNotPresent".to_string(),
                ImagePullPolicy::Never => "Never".to_string(),
            })
    }

    fn ports(&self) -> Vec<ContainerPort> {
        let listeners = self.listeners();
        vec![
            ContainerPort {
                name: Some(PORT_NAME_INTERNAL.to_string()),
                container_port: listeners.internal.port,
                protocol: Some("TCP".to_string()),
                ..Default::default()
            },
            ContainerPort {
                name: Some("client".to_string()),
                container_port: listeners.client.port,
                protocol: Some("TCP".to_string()),
                ..Default::default()
            },
        ]
    }

    /// Resource requirements straight from the component spec.
    ///
    /// No policy here: requests/limits are the user's, carried verbatim into
    /// Kubernetes quantities. JVM heap sizing (7vp7) stays out.
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
        let bind = format!(
            "{}://$POD_IP:{}, {}://$POD_IP:{}",
            listeners.internal.name,
            listeners.internal.port,
            listeners.client.name,
            listeners.client.port,
        );
        let advertised = format!(
            "{}://{}:{}",
            listeners.client.name, dns, listeners.client.port
        );
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
            env.push(EnvVar {
                name: "READINESS_TCP_PORT".to_string(),
                value: Some(self.listeners().client.port.to_string()),
                ..Default::default()
            });
        }
        env
    }

    fn volumes(&self) -> Vec<Volume> {
        let mut volumes = vec![Volume {
            name: CONFIG_VOLUME.to_string(),
            config_map: Some(ConfigMapVolumeSource {
                name: format!("{}{}", self.cluster_name, self.role.config_map_suffix()),
                ..Default::default()
            }),
            ..Default::default()
        }];
        if let S3AuthenticationSpec::Secret { secret_ref } =
            &self.cluster.spec.remote_storage.s3.authentication
        {
            // The server.yaml markers resolve against these exact file names;
            // the Secret's own key names are mapped, never required to match.
            volumes.push(Volume {
                name: S3_CREDENTIALS_VOLUME.to_string(),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(secret_ref.name.clone()),
                    items: Some(vec![
                        KeyToPath {
                            key: secret_ref.access_key_key.clone(),
                            path: S3_ACCESS_KEY_FILE.to_string(),
                            ..Default::default()
                        },
                        KeyToPath {
                            key: secret_ref.secret_key_key.clone(),
                            path: S3_SECRET_KEY_FILE.to_string(),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        if self.role == Role::Tablet {
            // Placeholder until 7vp7 renders volumeClaimTemplates; the mount
            // path already matches the tablet server.yaml data directory.
            volumes.push(Volume {
                name: DATA_VOLUME.to_string(),
                empty_dir: Some(EmptyDirVolumeSource::default()),
                ..Default::default()
            });
        }
        volumes
    }

    fn mounts(&self) -> Vec<VolumeMount> {
        let mut mounts = vec![VolumeMount {
            name: CONFIG_VOLUME.to_string(),
            mount_path: STAGING_DIR.to_string(),
            ..Default::default()
        }];
        if matches!(
            self.cluster.spec.remote_storage.s3.authentication,
            S3AuthenticationSpec::Secret { .. }
        ) {
            mounts.push(VolumeMount {
                name: S3_CREDENTIALS_VOLUME.to_string(),
                mount_path: S3_SECRETS_DIR.to_string(),
                read_only: Some(true),
                ..Default::default()
            });
        }
        if self.role == Role::Tablet {
            mounts.push(VolumeMount {
                name: DATA_VOLUME.to_string(),
                mount_path: server_config::storage::data_dir(self.cluster),
                ..Default::default()
            });
        }
        mounts
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
                field_path: field_path.to_string(),
                ..Default::default()
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
        failure_threshold: Some(failure_threshold),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{desired_coordinator_statefulset, desired_tablet_statefulset};
    use crate::constants::CONFIG_HASH_ANNOTATION;
    use crate::resources::config_map;
    use crate::utils::hash;

    fn spike_cluster() -> crate::api::FlussCluster {
        let mut cluster: crate::api::FlussCluster =
            serde_yaml::from_str(include_str!("../../../lab2/demo.yml"))
                .expect("lab2 demo CR must deserialize");
        cluster.metadata.uid = Some("test-uid".to_string());
        cluster
    }

    fn pod_spec(
        sts: &k8s_openapi::api::apps::v1::StatefulSet,
    ) -> &k8s_openapi::api::core::v1::PodSpec {
        sts.spec
            .as_ref()
            .expect("statefulset needs a spec")
            .template
            .spec
            .as_ref()
            .expect("pod template needs a spec")
    }

    fn container(
        sts: &k8s_openapi::api::apps::v1::StatefulSet,
    ) -> &k8s_openapi::api::core::v1::Container {
        assert_eq!(
            pod_spec(sts).containers.len(),
            1,
            "exactly one container per pod"
        );
        &pod_spec(sts).containers[0]
    }

    #[test]
    fn coordinator_statefulset_links_service_config_and_hash() {
        let cluster = spike_cluster();
        let sts = desired_coordinator_statefulset(&cluster).expect("valid CR must render");
        let spec = sts.spec.as_ref().expect("statefulset needs a spec");

        assert_eq!(
            spec.service_name.as_deref(),
            Some("spike-coordinator-headless")
        );
        assert_eq!(spec.replicas, Some(1));

        let script = container(&sts)
            .command
            .as_ref()
            .expect("container needs a command")[2]
            .clone();
        assert!(script.contains("cp /opt/operator-conf/server.yaml $FLUSS_HOME/conf"));
        assert!(script.contains("exec bin/coordinator-server.sh start-foreground"));
        assert!(script.contains("bind.listeners:"));
        assert!(script.contains("advertised.listeners:"));
        assert!(
            !script.contains("tablet-server.id"),
            "coordinator takes no tablet id"
        );

        let readiness = container(&sts)
            .readiness_probe
            .as_ref()
            .expect("coordinator needs a readiness probe");
        assert!(
            readiness.exec.is_none(),
            "coordinator declares GREEN; TCP is the honest signal"
        );
        assert_eq!(
            readiness
                .tcp_socket
                .as_ref()
                .expect("coordinator readiness is TCP")
                .port,
            k8s_openapi::apimachinery::pkg::util::intstr::IntOrString::Int(9124),
        );

        let yaml = config_map::coordinator_server_yaml(&cluster).expect("same render must hold");
        let annotations = spec
            .template
            .metadata
            .as_ref()
            .expect("pod template needs metadata")
            .annotations
            .as_ref()
            .expect("pod template needs annotations");
        assert_eq!(
            annotations.get(CONFIG_HASH_ANNOTATION).map(String::as_str),
            Some(hash::sha256_hex(&yaml).as_str()),
            "pod template pins the rendered coordinator config"
        );
    }

    #[test]
    fn tablet_statefulset_carries_ordinal_identity_and_probes() {
        let cluster = spike_cluster();
        let sts = desired_tablet_statefulset(&cluster).expect("valid CR must render");
        let spec = sts.spec.as_ref().expect("statefulset needs a spec");

        assert_eq!(
            spec.service_name.as_deref(),
            Some("spike-tabletserver-headless")
        );
        assert_eq!(spec.replicas, Some(3));

        let script = container(&sts)
            .command
            .as_ref()
            .expect("container needs a command")[2]
            .clone();
        assert!(script.contains("${POD_NAME##*-}"), "ordinal from pod name");
        assert!(script.contains("tablet-server.id: ${FLUSS_SERVER_ID}"));
        assert!(script.contains("INTERNAL://$POD_IP:9123"), "bind to pod IP");
        assert!(
            script.contains("CLIENT://$POD_NAME.spike-tabletserver-headless."),
            "advertise stable pod DNS"
        );
        assert!(script.contains("exec bin/tablet-server.sh start-foreground"));

        let mount = pod_spec(&sts)
            .volumes
            .as_ref()
            .expect("tablet needs volumes")
            .iter()
            .find(|v| v.name == "data")
            .expect("tablet needs a data volume");
        assert!(mount.empty_dir.is_some(), "emptyDir until 7vp7 PVCs");

        let credentials = pod_spec(&sts)
            .volumes
            .as_ref()
            .expect("tablet needs volumes")
            .iter()
            .find(|v| v.name == "s3-credentials")
            .expect("secret auth mounts the S3 Secret");
        let secret = credentials.secret.as_ref().expect("s3 volume is a secret");
        assert_eq!(secret.secret_name.as_deref(), Some("placeholder"));
        let paths: Vec<&str> = secret
            .items
            .as_ref()
            .expect("secret keys are mapped")
            .iter()
            .map(|item| item.path.as_str())
            .collect();
        assert!(paths.contains(&"access-key"));
        assert!(paths.contains(&"secret-key"));

        let readiness = container(&sts)
            .readiness_probe
            .as_ref()
            .expect("tablet needs a readiness probe");
        let command = readiness
            .exec
            .as_ref()
            .expect("tablet readiness is exec")
            .command
            .as_ref()
            .expect("probe needs a command");
        assert_eq!(
            command,
            &vec!["/opt/fluss/bin/readiness-check.sh".to_string()]
        );

        let yaml = config_map::tablet_server_yaml(&cluster).expect("same render must hold");
        let annotations = spec
            .template
            .metadata
            .as_ref()
            .expect("pod template needs metadata")
            .annotations
            .as_ref()
            .expect("pod template needs annotations");
        assert_eq!(
            annotations.get(CONFIG_HASH_ANNOTATION).map(String::as_str),
            Some(hash::sha256_hex(&yaml).as_str()),
            "pod template pins the rendered tablet config"
        );
    }
}
