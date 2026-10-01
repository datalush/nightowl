// SPDX-License-Identifier: AGPL-3.0-only
//! Desired Coordinator / TabletServer StatefulSets.
//!
//! One file per concern under [`statefulset`](self): `container` (identity
//! boot script, probes, resources), `volumes` (staged config, credentials,
//! data), `pod` (template metadata, placement, linkage). This file only
//! assembles: role inputs, shared names, and the final object.
//!
//! The boot sequence and probe budgets mirror the proven Helm reference
//! running in the lab (`fluss` namespace): bind to pod IP, advertise stable
//! pod DNS, TCP liveness on the client port, and the image's own
//! `readiness-check.sh` (local TCP + cluster-health GREEN gate) for tablet
//! readiness.

mod container;
mod pod;
mod tls;
mod volumes;
use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{StatefulSet, StatefulSetSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta};

use crate::api::FlussCluster;
use crate::constants::{
    COORDINATOR_CONFIG_SUFFIX, COORDINATOR_HEADLESS_SUFFIX, COORDINATOR_STATEFULSET_SUFFIX,
    LABEL_CLUSTER, LABEL_ROLE, ROLE_COORDINATOR, ROLE_TABLET, TABLET_CONFIG_SUFFIX,
    TABLET_HEADLESS_SUFFIX, TABLET_STATEFULSET_SUFFIX,
};
use crate::utils::hash;
use crate::utils::render;

use super::config_map;
use super::server_config::ConfigError;
use super::server_config::dynamic;
use super::server_config::metrics;

/// Staging path for the mounted ConfigMap: the image rewrites its own
/// `server.yaml` at boot, so the read-only ConfigMap mount must land
/// elsewhere and be copied over, never mounted in place.
pub(super) const STAGING_DIR: &str = "/opt/operator-conf";

/// Desired Coordinator StatefulSet, with its config hash pinned in the pod
/// template so rendered config changes roll the pods.
pub fn desired_coordinator_statefulset(
    cluster: &FlussCluster,
    secret_hash: Option<&str>,
) -> Result<StatefulSet, ConfigError> {
    Build::assemble(cluster, Role::Coordinator, secret_hash)?.build()
}

/// Desired TabletServer StatefulSet. See [`desired_coordinator_statefulset`].
pub fn desired_tablet_statefulset(
    cluster: &FlussCluster,
    secret_hash: Option<&str>,
) -> Result<StatefulSet, ConfigError> {
    Build::assemble(cluster, Role::Tablet, secret_hash)?.build()
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
/// consumed method by method — so no function here takes more than two
/// arguments and the shape stays uniform across roles. The methods live
/// beside the concern they serve (`container`, `volumes`, `pod`).
struct Build<'a> {
    cluster: &'a FlussCluster,
    role: Role,
    cluster_name: String,
    service_name: String,
    image: String,
    labels: BTreeMap<String, String>,
    config_hash: String,
    secret_hash: Option<String>,
    /// Effective Prometheus scrape port, resolved once from the same merged
    /// properties the config hash covers — so annotations can never disagree
    /// with the rendered reporter. `None` means opted out or overridden away.
    scrape_port: Option<String>,
}

/// Desired static config hash for one role: the same recipe `assemble`
/// pins into the pod template. The restart sequencer reads it off live
/// pods to tell stale from current, so both must compute it identically —
/// keep them together here.
pub(crate) fn static_config_hash(
    cluster: &FlussCluster,
    coordinator: bool,
) -> Result<String, ConfigError> {
    let properties = match coordinator {
        true => config_map::coordinator_properties(cluster)?,
        false => config_map::tablet_properties(cluster)?,
    };
    let other = match coordinator {
        true => config_map::tablet_properties(cluster)?,
        false => config_map::coordinator_properties(cluster)?,
    };
    let appliable = dynamic::appliable(&properties, &other);
    let server_yaml = render::to_yaml(&dynamic::without_appliable(&properties, &appliable));
    // External identity is appended at boot, outside server.yaml. Include it
    // in rollout identity; replica changes deliberately do not change this hash.
    Ok(hash::sha256_hex(&super::external_access::rollout_input(
        cluster,
        coordinator,
        server_yaml,
    )))
}

impl<'a> Build<'a> {
    /// Resolve everything `build` needs. Rendering the `server.yaml` is the
    /// only fallible step, so it happens here, once. The secret hash arrives
    /// precomputed (the builder never touches the API); `None` means no
    /// secret auth or an unreadable secret, and pins nothing.
    fn assemble(
        cluster: &'a FlussCluster,
        role: Role,
        secret_hash: Option<&str>,
    ) -> Result<Self, ConfigError> {
        let cluster_name = cluster
            .metadata
            .name
            .clone()
            .expect("FlussCluster needs a name");
        let properties = match role {
            Role::Coordinator => config_map::coordinator_properties(cluster)?,
            Role::Tablet => config_map::tablet_properties(cluster)?,
        };
        // Rollout identity covers the static subset only: dynamic keys ride
        // Admin (see server_config::dynamic), so dynamic-only changes move
        // no pods. Computed from both roles because cluster-wide appliability
        // needs identical values on each side. Shared with the restart
        // sequencer through `static_config_hash` — one recipe, two readers.
        let config_hash = static_config_hash(cluster, matches!(role, Role::Coordinator))?;
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
            config_hash,
            secret_hash: secret_hash.map(str::to_string),
            scrape_port: metrics::scrape_port(&properties),
            cluster_name,
            image,
        })
    }

    fn build(&self) -> Result<StatefulSet, ConfigError> {
        Ok(StatefulSet {
            metadata: self.object_meta(),
            spec: Some(StatefulSetSpec {
                replicas: Some(self.replicas()),
                selector: LabelSelector {
                    match_labels: Some(self.labels.clone()),
                    ..Default::default()
                },
                service_name: Some(self.service_name.clone()),
                template: self.pod_template()?,
                volume_claim_templates: self.claims(),
                // OnDelete on purpose: template updates (config hash,
                // image) must never roll pods behind the restart
                // sequencer's back. Every restart is a sequenced,
                // health-gated pod delete (j5v3); Kubernetes only
                // recreates what the sequencer deletes.
                update_strategy: Some(k8s_openapi::api::apps::v1::StatefulSetUpdateStrategy {
                    type_: Some("OnDelete".to_string()),
                    rolling_update: None,
                }),
                persistent_volume_claim_retention_policy: Some(
                    k8s_openapi::api::apps::v1::StatefulSetPersistentVolumeClaimRetentionPolicy {
                        when_deleted: Some("Retain".to_string()),
                        when_scaled: Some("Retain".to_string()),
                    },
                ),
                ..Default::default()
            }),
            status: None,
        })
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
        cluster.spec.coordinator.jvm = Some(crate::api::JvmSpec {
            heap: "512Mi".to_string(),
            extra_args: vec![],
        });
        cluster.spec.tablet_servers.jvm = Some(crate::api::JvmSpec {
            heap: "1Gi".to_string(),
            extra_args: vec!["-Dfoo=bar".to_string()],
        });
        cluster.spec.tablet_servers.scheduling = Some(crate::api::SchedulingSpec {
            spread_across_nodes: true,
            node_selector: std::collections::BTreeMap::new(),
            affinity: None,
            tolerations: vec![],
            topology_spread_constraints: vec![],
        });
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
        let sts = desired_coordinator_statefulset(&cluster, Some("sha256:abc"))
            .expect("valid CR must render");
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
        assert!(
            yaml.contains("env.java.opts.coordinator-server: -Xms512M -Xmx512M"),
            "heap normalized from Kubernetes to JVM units, got:\n{yaml}"
        );
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
    fn grace_period_comes_from_rolling_upgrade_or_default() {
        let cluster = spike_cluster();
        let sts = desired_tablet_statefulset(&cluster, None).expect("valid CR must render");
        let grace = sts
            .spec
            .as_ref()
            .expect("statefulset needs a spec")
            .template
            .spec
            .as_ref()
            .expect("pod template needs a spec")
            .termination_grace_period_seconds;
        assert_eq!(
            grace,
            Some(30),
            "absent rollingUpgrade means the K8s default"
        );

        let mut cluster = spike_cluster();
        cluster.spec.rolling_upgrade = Some(crate::api::RollingUpgradeSpec {
            controlled_shutdown_timeout: "2min".to_string(),
            recovery_timeout: "5min".to_string(),
            stabilization_window: "1min".to_string(),
        });
        let sts = desired_tablet_statefulset(&cluster, None).expect("valid CR must render");
        let grace = sts
            .spec
            .as_ref()
            .expect("statefulset needs a spec")
            .template
            .spec
            .as_ref()
            .expect("pod template needs a spec")
            .termination_grace_period_seconds;
        assert_eq!(grace, Some(120), "controlled shutdown budget becomes grace");

        cluster.spec.rolling_upgrade = Some(crate::api::RollingUpgradeSpec {
            controlled_shutdown_timeout: "soon".to_string(),
            recovery_timeout: "5min".to_string(),
            stabilization_window: "1min".to_string(),
        });
        assert!(
            desired_tablet_statefulset(&cluster, None).is_err(),
            "unparsable timeouts fail closed instead of guessing a grace"
        );
    }

    #[test]
    fn tablet_statefulset_carries_ordinal_identity_and_probes() {
        let cluster = spike_cluster();
        let sts =
            desired_tablet_statefulset(&cluster, Some("sha256:abc")).expect("valid CR must render");
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
            .find(|v| v.name == "data");
        assert!(
            mount.is_none(),
            "claim template owns the data volume, no emptyDir alongside"
        );
        let claims = sts
            .spec
            .as_ref()
            .expect("statefulset needs a spec")
            .volume_claim_templates
            .as_ref()
            .expect("tablet storage renders a claim template");
        assert_eq!(claims.len(), 1);
        let claim = claims[0].spec.as_ref().expect("claim needs a spec");
        assert_eq!(
            claim
                .resources
                .as_ref()
                .expect("claim needs resources")
                .requests
                .as_ref()
                .expect("claim needs requests")["storage"]
                .0,
            "5Gi",
            "size from the CR"
        );

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
        assert!(
            yaml.contains("env.java.opts.tablet-server: -Xms1G -Xmx1G -Dfoo=bar"),
            "heap plus extra args land in server.yaml, got:\n{yaml}"
        );

        let spread = pod_spec(&sts)
            .topology_spread_constraints
            .as_ref()
            .expect("spreadAcrossNodes renders a constraint");
        assert!(
            spread
                .iter()
                .any(|c| c.topology_key == "kubernetes.io/hostname"
                    && c.when_unsatisfiable == "ScheduleAnyway"),
            "soft hostname spread from the CR"
        );
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
        assert_eq!(
            annotations
                .get(crate::constants::SECRET_HASH_ANNOTATION)
                .map(String::as_str),
            Some("sha256:abc"),
            "pod template pins the secret hash for rotation detection"
        );
    }

    fn template_annotations(
        sts: &k8s_openapi::api::apps::v1::StatefulSet,
    ) -> &std::collections::BTreeMap<String, String> {
        sts.spec
            .as_ref()
            .expect("statefulset needs a spec")
            .template
            .metadata
            .as_ref()
            .expect("pod template needs metadata")
            .annotations
            .as_ref()
            .expect("pod template needs annotations")
    }

    #[test]
    fn scrape_annotations_follow_the_default_on_reporter() {
        // demo.yml sets no observability: default-on renders the reporter key
        // and both roles carry scrape annotations on the default port.
        let cluster = spike_cluster();
        for sts in [
            desired_coordinator_statefulset(&cluster, None).expect("valid CR must render"),
            desired_tablet_statefulset(&cluster, None).expect("valid CR must render"),
        ] {
            let annotations = template_annotations(&sts);
            assert_eq!(
                annotations
                    .get(crate::resources::server_config::metrics::ANNOTATION_SCRAPE)
                    .map(String::as_str),
                Some("true")
            );
            assert_eq!(
                annotations
                    .get(crate::resources::server_config::metrics::ANNOTATION_PORT)
                    .map(String::as_str),
                Some(crate::resources::server_config::metrics::DEFAULT_PORT)
            );
        }
        let yaml = config_map::tablet_server_yaml(&cluster).expect("same render must hold");
        assert!(
            yaml.contains("metrics.reporters: prometheus"),
            "reporter key rendered by default, got:\n{yaml}"
        );
    }

    #[test]
    fn explicit_opt_out_removes_reporter_and_annotations() {
        let mut cluster = spike_cluster();
        cluster.spec.observability = Some(crate::api::ObservabilitySpec { prometheus: false });
        let sts = desired_tablet_statefulset(&cluster, None).expect("valid CR must render");
        let annotations = template_annotations(&sts);
        assert!(
            annotations
                .get(crate::resources::server_config::metrics::ANNOTATION_SCRAPE)
                .is_none(),
            "opted-out pods must not advertise scraping"
        );
        let yaml = config_map::tablet_server_yaml(&cluster).expect("same render must hold");
        assert!(
            !yaml.contains("metrics.reporters:"),
            "no reporter key when opted out, got:\n{yaml}"
        );
    }

    #[test]
    fn custom_reporter_port_reaches_the_annotations() {
        let mut cluster = spike_cluster();
        cluster.spec.configuration_overrides.insert(
            crate::resources::server_config::metrics::PORT_KEY.to_string(),
            "9250".to_string(),
        );
        let sts = desired_tablet_statefulset(&cluster, None).expect("valid CR must render");
        assert_eq!(
            template_annotations(&sts)
                .get(crate::resources::server_config::metrics::ANNOTATION_PORT)
                .map(String::as_str),
            Some("9250"),
            "user port override wins and stays consistent with scraping"
        );
    }

    #[test]
    fn dynamic_only_change_moves_no_pods() {
        use crate::resources::server_config::dynamic;
        use crate::utils::hash;
        use crate::utils::render;

        let plain = spike_cluster();
        let plain_sts = desired_tablet_statefulset(&plain, None).expect("valid CR must render");
        let plain_hash = template_annotations(&plain_sts)
            .get(crate::constants::CONFIG_HASH_ANNOTATION)
            .expect("template pins a hash");

        let mut changed = spike_cluster();
        changed
            .spec
            .configuration_overrides
            .insert("kv.snapshot.interval".to_string(), "30s".to_string());
        let changed_sts = desired_tablet_statefulset(&changed, None).expect("valid CR must render");
        let changed_hash = template_annotations(&changed_sts)
            .get(crate::constants::CONFIG_HASH_ANNOTATION)
            .expect("template pins a hash");
        assert_eq!(
            changed_hash, plain_hash,
            "dynamic-only diffs must not change the rollout hash"
        );

        // And the hash that did stay still is the static subset, not luck:
        // the full yaml does contain the new value.
        let full = config_map::tablet_server_yaml(&changed).expect("same render must hold");
        assert!(full.contains("kv.snapshot.interval: 30s"));
        let coord = config_map::coordinator_properties(&changed).expect("props must render");
        let tablet = config_map::tablet_properties(&changed).expect("props must render");
        let appliable = dynamic::appliable(&coord, &tablet);
        let expected = hash::sha256_hex(&render::to_yaml(&dynamic::without_appliable(
            &tablet, &appliable,
        )));
        assert_eq!(changed_hash, &expected);
    }
}
