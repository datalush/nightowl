// SPDX-License-Identifier: AGPL-3.0-only
//! Pod shape: template metadata, placement, and identity linkage.
//!
//! User pod labels/annotations ride along (ours win on collision) while the
//! StatefulSet selector stays fixed, so extra template labels can never
//! break pod matching. Scheduling intent passes straight from the spec;
//! `spreadAcrossNodes` adds a soft hostname spread that keeps single-node
//! labs working while spreading real clusters.

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    LocalObjectReference, PodSpec, PodTemplateSpec, TopologySpreadConstraint,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};

use super::{Build, Role};
use crate::api::PodTemplateSpec as FlussPodTemplate;
use crate::api::{ImagePullPolicy, ListenersSpec, SchedulingSpec};
use crate::constants::{
    API_VERSION, CONFIG_HASH_ANNOTATION, KIND_FLUSS_CLUSTER, SECRET_HASH_ANNOTATION,
};
use crate::resources::server_config::metrics;

impl<'a> Build<'a> {
    pub(super) fn pod_template(
        &self,
    ) -> Result<PodTemplateSpec, crate::resources::server_config::ConfigError> {
        let overlay = self.pod_overlay();
        let mut template_labels = self.labels.clone();
        let mut template_annotations = BTreeMap::new();
        if let Some(t) = overlay {
            template_labels.extend(t.labels.clone());
            template_annotations.extend(t.annotations.clone());
        }
        template_annotations.insert(CONFIG_HASH_ANNOTATION.to_string(), self.config_hash.clone());
        // Prometheus scrape follows the effective reporter port resolved at
        // assemble time: absent exactly when nothing listens. Operator keys
        // win on collision, like every other annotation here.
        if let Some(port) = &self.scrape_port {
            template_annotations.insert(metrics::ANNOTATION_SCRAPE.to_string(), "true".to_string());
            template_annotations.insert(metrics::ANNOTATION_PORT.to_string(), port.clone());
        }
        // Pinned for staleness detection only; the comparator ignores it so
        // rotation reports instead of rolling (restart policy is separate).
        if let Some(secret_hash) = &self.secret_hash {
            template_annotations.insert(SECRET_HASH_ANNOTATION.to_string(), secret_hash.clone());
        }

        Ok(PodTemplateSpec {
            metadata: Some(ObjectMeta {
                labels: Some(template_labels),
                annotations: Some(template_annotations),
                ..Default::default()
            }),
            spec: Some(PodSpec {
                containers: vec![self.container()],
                volumes: Some(self.volumes()),
                security_context: overlay.and_then(|t| t.security_context.clone()),
                image_pull_secrets: {
                    let secrets: Vec<LocalObjectReference> = self
                        .cluster
                        .spec
                        .image
                        .pull_secrets
                        .iter()
                        .map(|name| LocalObjectReference { name: name.clone() })
                        .collect();
                    // Absent when empty: an empty list reads back nulled and
                    // would look like drift on every trigger.
                    (!secrets.is_empty()).then_some(secrets)
                },
                node_selector: self
                    .scheduling()
                    .and_then(|s| (!s.node_selector.is_empty()).then(|| s.node_selector.clone())),
                affinity: self.scheduling().and_then(|s| s.affinity.clone()),
                tolerations: self
                    .scheduling()
                    .and_then(|s| (!s.tolerations.is_empty()).then(|| s.tolerations.clone())),
                topology_spread_constraints: self.spread_constraints(),
                termination_grace_period_seconds: Some(self.grace_period_seconds()?),
                ..Default::default()
            }),
        })
    }

    /// Seconds the kubelet waits after SIGTERM before SIGKILL.
    ///
    /// Fluss handles SIGTERM with an internal controlled shutdown; the
    /// grace period is what lets it finish. From
    /// `rollingUpgrade.controlledShutdownTimeout`, else the Kubernetes
    /// 30s default rendered explicitly (an absent field reads back
    /// defaulted and would look like drift). Unparsable values fail
    /// closed: a guess here could SIGKILL mid-shutdown.
    fn grace_period_seconds(&self) -> Result<i64, crate::resources::server_config::ConfigError> {
        use crate::resources::server_config::ConfigError;
        use crate::utils::duration;

        match self
            .cluster
            .spec
            .rolling_upgrade
            .as_ref()
            .map(|upgrade| upgrade.controlled_shutdown_timeout.as_str())
        {
            None => Ok(30),
            Some(raw) => duration::to_seconds(raw)
                .and_then(|seconds| seconds.try_into().ok())
                .ok_or_else(|| ConfigError::InvalidValue {
                    key: "rollingUpgrade.controlledShutdownTimeout".to_string(),
                    value: raw.to_string(),
                    reason: "must be a Fluss duration like 30s, 5min or 1h".to_string(),
                }),
        }
    }

    fn pod_overlay(&self) -> Option<&FlussPodTemplate> {
        match self.role {
            Role::Coordinator => self.cluster.spec.coordinator.pod_template.as_ref(),
            Role::Tablet => self.cluster.spec.tablet_servers.pod_template.as_ref(),
        }
    }

    /// Component scheduling intent, when the CR sets one.
    fn scheduling(&self) -> Option<&SchedulingSpec> {
        match self.role {
            Role::Coordinator => self.cluster.spec.coordinator.scheduling.as_ref(),
            Role::Tablet => self.cluster.spec.tablet_servers.scheduling.as_ref(),
        }
    }

    /// Spread constraints: the CR's own plus one hostname-spread when
    /// `spreadAcrossNodes` asks for it.
    ///
    /// `ScheduleAnyway` (not `DoNotSchedule`): a hard hostname spread can
    /// strand pods when zones/nodes are fewer than replicas; soft spread
    /// keeps single-node labs working while spreading real clusters.
    fn spread_constraints(&self) -> Option<Vec<TopologySpreadConstraint>> {
        let scheduling = self.scheduling()?;
        let mut constraints = scheduling.topology_spread_constraints.clone();
        if scheduling.spread_across_nodes {
            constraints.push(TopologySpreadConstraint {
                max_skew: 1,
                topology_key: "kubernetes.io/hostname".to_string(),
                when_unsatisfiable: "ScheduleAnyway".to_string(),
                label_selector: Some(LabelSelector {
                    match_labels: Some(self.labels.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        (!constraints.is_empty()).then_some(constraints)
    }

    pub(super) fn owner_reference(&self) -> OwnerReference {
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

    /// Pull policy straight from the spec, defaulting to what Kubernetes
    /// itself would do for our versioned images.
    ///
    /// Always set (never absent): an absent field reads back defaulted and
    /// would look like drift on every trigger. `IfNotPresent` is the
    /// Kubernetes default for non-`latest` tags, which is all we ever run.
    pub(super) fn image_pull_policy(&self) -> Option<String> {
        Some(
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
                .unwrap_or_else(|| "IfNotPresent".to_string()),
        )
    }

    pub(super) fn replicas(&self) -> i32 {
        match self.role {
            Role::Coordinator => self.cluster.spec.coordinator.replicas,
            Role::Tablet => self.cluster.spec.tablet_servers.replicas,
        }
    }

    pub(super) fn listeners(&self) -> &ListenersSpec {
        self.cluster
            .spec
            .listeners
            .as_ref()
            .expect("listeners is required for the StatefulSet pods")
    }
}
