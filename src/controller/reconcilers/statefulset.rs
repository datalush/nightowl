//! Reconcile both StatefulSets (coordinator + tablet) and report what happened.
//!
//! Mirrors `config_map.rs`: one [`Observation`] per StatefulSet, builder
//! failures and foreign owners become `StatefulSetBlocked` observations so
//! `.status` documents the block instead of hiding it.
//!
//! Bring-up order is explicit: the tablet StatefulSet waits for a ready
//! coordinator replica instead of crashlooping against a coordinator that
//! does not serve yet. The coordinator's own status flips retrigger this
//! controller through `owns()`, so no polling is needed; and the gate only
//! ever delays creation — updates, rollouts and recovery flow ungated, so a
//! coordinator that never readies blocks tablets instead of wedging them.

use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::{
    Container, PersistentVolumeClaim, PodSpec, PodTemplateSpec, Secret, Volume,
};
use k8s_openapi::api::storage::v1::StorageClass;
use kube::Api;

use super::Observation;
use crate::api::FlussCluster;
use crate::constants::{COORDINATOR_STATEFULSET_SUFFIX, TABLET_STATEFULSET_SUFFIX};
use crate::controller::Error;
use crate::controller::apply;
use crate::resources::statefulset as builder;

/// Converge both StatefulSets toward the desired state.
///
/// Tablets wait for coordinator readiness (see module docs); the wait is
/// reported, not hidden, and never an error.
pub async fn reconcile(
    statefulsets: &Api<StatefulSet>,
    secrets: &Api<Secret>,
    pvcs: &Api<PersistentVolumeClaim>,
    storage_classes: &Api<StorageClass>,
    cluster: &FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    // Pinned into fresh pod templates below; compared against the pins
    // afterwards for rotation detection. `None` under workload identity or
    // when the Secret cannot be read (the storage guardrail reports that).
    let live_hash = live_secret_hash(secrets, cluster).await?;
    let mut observations = Vec::with_capacity(6);
    observations.extend(
        converge_role(
            statefulsets,
            pvcs,
            storage_classes,
            cluster,
            uid,
            Role::Coordinator,
            live_hash.as_deref(),
        )
        .await?,
    );
    if coordinator_ready(statefulsets, cluster).await? {
        observations.extend(
            converge_role(
                statefulsets,
                pvcs,
                storage_classes,
                cluster,
                uid,
                Role::Tablet,
                live_hash.as_deref(),
            )
            .await?,
        );
    } else {
        let name = format!(
            "{}{}",
            cluster.metadata.name.clone().ok_or(Error::MissingName)?,
            Role::Tablet.suffix()
        );
        tracing::info!(statefulset = %name, "waiting for a ready coordinator replica");
        observations.push(Observation::WaitingForCoordinator { name });
    }
    observations.extend(detect_stale(statefulsets, cluster, live_hash.as_deref()).await?);
    Ok(observations)
}

/// True once the coordinator StatefulSet reports a ready replica.
///
/// Missing object means "not yet converged this run" — the coordinator step
/// above just created it. API failures propagate (transient, requeued); a
/// coordinator that never readies simply holds tablets, it never wedges
/// them.
async fn coordinator_ready(api: &Api<StatefulSet>, cluster: &FlussCluster) -> Result<bool, Error> {
    let name = format!(
        "{}{}",
        cluster.metadata.name.clone().ok_or(Error::MissingName)?,
        Role::Coordinator.suffix()
    );
    match api.get(&name).await {
        Ok(coordinator) => Ok(coordinator
            .status
            .as_ref()
            .and_then(|status| status.ready_replicas)
            .unwrap_or(0)
            > 0),
        Err(kube::Error::Api(status)) if status.code == 404 => Ok(false),
        Err(e) => Err(Error::Kube(e)),
    }
}

/// Hash of the live S3 Secret content, or `None` when there is nothing to
/// pin: workload identity uses no secret, and a missing or key-incomplete
/// secret is already reported by the storage guardrail.
///
/// Only the referenced keys participate, so unrelated keys in the same
/// Secret never read as rotation.
async fn live_secret_hash(
    secrets: &Api<Secret>,
    cluster: &FlussCluster,
) -> Result<Option<String>, Error> {
    use crate::api::S3AuthenticationSpec;
    use crate::utils::hash;

    let secret_ref = match &cluster.spec.remote_storage.s3.authentication {
        S3AuthenticationSpec::Secret { secret_ref } => secret_ref,
        S3AuthenticationSpec::WorkloadIdentity { .. } => return Ok(None),
    };
    let secret = match secrets.get(&secret_ref.name).await {
        Ok(secret) => secret,
        Err(kube::Error::Api(status)) if status.code == 404 => return Ok(None),
        Err(e) => return Err(Error::Kube(e)),
    };
    let data: std::collections::BTreeMap<String, Vec<u8>> = secret
        .data
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key, value.0))
        .collect();
    Ok(hash::secret_data_hash(
        &data,
        &[
            secret_ref.access_key_key.clone(),
            secret_ref.secret_key_key.clone(),
        ],
    ))
}

/// Compare the live Secret hash against what each StatefulSet's pods
/// mounted (pinned in their template at render time).
///
/// Emits at most one observation per role: `SecretStale` naming the
/// affected pod ordinals and the stale mount when both hashes exist and
/// differ, `SecretFresh` when they agree or when no secret auth exists to
/// go stale. Unreadable secrets emit nothing — absence is honest, and the
/// storage guardrail already covers missing secrets.
/// Detection only: never an error, never a rollout.
async fn detect_stale(
    statefulsets: &Api<StatefulSet>,
    cluster: &FlussCluster,
    live_hash: Option<&str>,
) -> Result<Vec<Observation>, Error> {
    use crate::api::S3AuthenticationSpec;
    use crate::constants::{S3_SECRETS_DIR, SECRET_HASH_ANNOTATION};

    if !matches!(
        cluster.spec.remote_storage.s3.authentication,
        S3AuthenticationSpec::Secret { .. }
    ) {
        return Ok(vec![Observation::SecretFresh {
            message: "no secret authentication configured; nothing to go stale".to_string(),
        }]);
    }
    let Some(live_hash) = live_hash else {
        return Ok(Vec::new());
    };
    let mut observations = Vec::new();
    let mut fresh = true;
    for role in [Role::Coordinator, Role::Tablet] {
        let name = format!(
            "{}{}",
            cluster.metadata.name.clone().ok_or(Error::MissingName)?,
            role.suffix()
        );
        let live = match statefulsets.get(&name).await {
            Ok(live) => live,
            Err(kube::Error::Api(status)) if status.code == 404 => continue,
            Err(e) => return Err(Error::Kube(e)),
        };
        let pinned = live
            .spec
            .as_ref()
            .and_then(|spec| spec.template.metadata.as_ref())
            .and_then(|meta| meta.annotations.as_ref())
            .and_then(|annotations| annotations.get(SECRET_HASH_ANNOTATION));
        match pinned {
            Some(pinned) if pinned != live_hash => {
                fresh = false;
                let replicas = live
                    .spec
                    .as_ref()
                    .and_then(|spec| spec.replicas)
                    .unwrap_or_else(|| role.replicas(cluster));
                observations.push(Observation::SecretStale {
                    pods: role.pod_names(cluster, replicas),
                    message: format!(
                        "statefulset {name} pods mount a rotated secret; live content differs from the pinned {live_hash} (stale mount: {S3_SECRETS_DIR})"
                    ),
                });
            }
            _ => {}
        }
    }
    if fresh {
        observations.push(Observation::SecretFresh {
            message: "mounted secret content matches the live secret".to_string(),
        });
    }
    Ok(observations)
}

#[derive(Clone, Copy)]
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

    fn build(
        self,
        cluster: &FlussCluster,
        secret_hash: Option<&str>,
    ) -> Result<StatefulSet, crate::resources::server_config::ConfigError> {
        match self {
            Role::Coordinator => builder::desired_coordinator_statefulset(cluster, secret_hash),
            Role::Tablet => builder::desired_tablet_statefulset(cluster, secret_hash),
        }
    }

    /// Ordinal pod names owned by this role's StatefulSet. Computed, never
    /// listed: no extra RBAC, and the names are stable by construction.
    fn pod_names(self, cluster: &FlussCluster, replicas: i32) -> Vec<String> {
        let base = format!(
            "{}{}",
            cluster.metadata.name.clone().unwrap_or_default(),
            self.suffix()
        );
        (0..replicas).map(|i| format!("{base}-{i}")).collect()
    }

    fn replicas(self, cluster: &FlussCluster) -> i32 {
        match self {
            Role::Coordinator => cluster.spec.coordinator.replicas,
            Role::Tablet => cluster.spec.tablet_servers.replicas,
        }
    }
}

async fn converge_role(
    statefulsets: &Api<StatefulSet>,
    pvcs: &Api<PersistentVolumeClaim>,
    storage_classes: &Api<StorageClass>,
    cluster: &FlussCluster,
    uid: &str,
    role: Role,
    secret_hash: Option<&str>,
) -> Result<Vec<Observation>, Error> {
    // Storage lifecycle gates the template update: shrink, class change or
    // denied growth refuse with evidence instead of converging. Growth
    // patches the live PVCs first (in the check itself), so the template
    // update that follows only carries the new size to future pods.
    let volume_role = match role {
        Role::Coordinator => super::volume::Role::Coordinator,
        Role::Tablet => super::volume::Role::Tablet,
    };
    if let Some(observation) =
        super::volume::check(statefulsets, pvcs, storage_classes, cluster, volume_role).await?
    {
        if matches!(observation, Observation::VolumeBlocked { .. }) {
            return Ok(vec![observation]);
        }
        let mut observations = vec![observation];
        observations.push(converge_one(statefulsets, cluster, uid, role, secret_hash).await?);
        return Ok(observations);
    }
    Ok(vec![
        converge_one(statefulsets, cluster, uid, role, secret_hash).await?,
    ])
}

async fn converge_one(
    api: &Api<StatefulSet>,
    cluster: &FlussCluster,
    uid: &str,
    role: Role,
    secret_hash: Option<&str>,
) -> Result<Observation, Error> {
    let name = format!(
        "{}{}",
        cluster.metadata.name.clone().ok_or(Error::MissingName)?,
        role.suffix()
    );
    let desired = match role.build(cluster, secret_hash) {
        Ok(desired) => desired,
        Err(e) => {
            return Ok(Observation::StatefulSetBlocked {
                name,
                message: e.to_string(),
            });
        }
    };
    // Fail-closed scale-in: the only policy is Block, and emptiness is not
    // observable until the per-server read API exists (erbh). A decrement
    // therefore never converges — it reports instead. Scale-out flows
    // through untouched.
    if matches!(role, Role::Tablet) {
        let wanted = desired
            .spec
            .as_ref()
            .and_then(|spec| spec.replicas)
            .unwrap_or(0);
        let live_replicas = match api.get(&name).await {
            Ok(live) => live.spec.as_ref().and_then(|spec| spec.replicas),
            Err(kube::Error::Api(status)) if status.code == 404 => None,
            Err(e) => return Err(Error::Kube(e)),
        };
        if let Some(live_replicas) = live_replicas
            && wanted < live_replicas
        {
            return Ok(Observation::StatefulSetBlocked {
                name: name.clone(),
                message: format!(
                    "refusing to scale tabletservers from {live_replicas} down to {wanted}: scale-in policy is Block and hosted replicas are not observable yet"
                ),
            });
        }
    }
    match apply::apply(api, desired, uid, same_statefulset).await {
        Ok(outcome) => Ok(Observation::StatefulSetConverged { name, outcome }),
        Err(Error::NotOwned(_)) => Ok(Observation::StatefulSetBlocked {
            name: name.clone(),
            message: format!(
                "statefulset {name} exists with a different owner; refusing to adopt it"
            ),
        }),
        Err(e) => Err(e),
    }
}

/// StatefulSets are the same when the fields the controller manages agree.
///
/// Pod templates compare recursively on managed fields only: the apiserver
/// defaults a long tail inside the template (volume `defaultMode`,
/// container termination message paths, pod `dnsPolicy`/`restartPolicy`,
/// the `default` service account and friends) that must never read as
/// drift, or every trigger rewrites the object.
fn same_statefulset(a: &StatefulSet, b: &StatefulSet) -> bool {
    let (Some(a_spec), Some(b_spec)) = (a.spec.as_ref(), b.spec.as_ref()) else {
        return a.spec.is_none() && b.spec.is_none();
    };
    a_spec.replicas == b_spec.replicas
        && a_spec.selector == b_spec.selector
        && same_template(&a_spec.template, &b_spec.template)
        && same_claims(
            a_spec.volume_claim_templates.as_deref(),
            b_spec.volume_claim_templates.as_deref(),
        )
        && a_spec.persistent_volume_claim_retention_policy
            == b_spec.persistent_volume_claim_retention_policy
}

/// Claim templates compare without `status` (the apiserver injects
/// `status.phase` into stored templates) and without size: the apiserver
/// forbids updating claim templates on a live StatefulSet at all, so size
/// growth converges the live PVCs directly (see `volume`) and the template
/// keeps the size new claims start from. Comparing size here would rewrite
/// forever against a rejection.
/// Claim templates compare without `status` and without size: the apiserver
/// injects `status.phase` into stored templates, and forbids updating claim
/// templates on a live StatefulSet at all. Size growth converges the live
/// PVCs directly (see `volume`); the template only sizes brand-new claims.
/// Comparing either would rewrite forever against a rejection.
fn same_claims(
    a: Option<&[k8s_openapi::api::core::v1::PersistentVolumeClaim]>,
    b: Option<&[k8s_openapi::api::core::v1::PersistentVolumeClaim]>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| same_claim(a, b))
        }
        _ => false,
    }
}

/// One claim template: metadata, access modes, class and mode. Excluded:
/// `status` (injected) and `resources.requests.storage` (grown in place;
/// the template never updates on a live object).
fn same_claim(
    a: &k8s_openapi::api::core::v1::PersistentVolumeClaim,
    b: &k8s_openapi::api::core::v1::PersistentVolumeClaim,
) -> bool {
    let spec = |claim: &k8s_openapi::api::core::v1::PersistentVolumeClaim| {
        claim.spec.as_ref().map(|spec| {
            (
                spec.access_modes.clone(),
                spec.storage_class_name.clone(),
                spec.volume_mode.clone(),
            )
        })
    };
    a.metadata == b.metadata && spec(a) == spec(b)
}

/// Template metadata is managed except the secret pin: labels plus our
/// annotations compare, but the secret hash is detection-only by design —
/// comparing it would roll pods on rotation, and restart policy is a
/// separate decision (j5v3).
fn same_template(a: &PodTemplateSpec, b: &PodTemplateSpec) -> bool {
    a.metadata.as_ref().map(metadata_key) == b.metadata.as_ref().map(metadata_key)
        && same_pod_spec(a.spec.as_ref(), b.spec.as_ref())
}

fn metadata_key(
    meta: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta,
) -> (
    Option<std::collections::BTreeMap<String, String>>,
    std::collections::BTreeMap<String, String>,
) {
    let mut annotations = meta.annotations.clone().unwrap_or_default();
    annotations.remove(crate::constants::SECRET_HASH_ANNOTATION);
    (meta.labels.clone(), annotations)
}

/// PodSpec fields the controller sets. Deliberately absent: `dnsPolicy`,
/// `restartPolicy`, `schedulerName`, `terminationGracePeriodSeconds`,
/// `serviceAccountName`, `enableServiceLinks` and every other server
/// default — comparing them would mistake every read-back for drift.
///
/// `securityContext` compares normalized: the apiserver persists an empty
/// object where the builder renders nothing, and `None` vs `{}` must read
/// as the same absence.
fn same_pod_spec(a: Option<&PodSpec>, b: Option<&PodSpec>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            same_containers(&a.containers, &b.containers)
                && same_volumes(a.volumes.as_deref(), b.volumes.as_deref())
                && same_security_context(a.security_context.as_ref(), b.security_context.as_ref())
                && a.image_pull_secrets == b.image_pull_secrets
                && a.node_selector == b.node_selector
                && a.affinity == b.affinity
                && a.tolerations == b.tolerations
                && a.topology_spread_constraints == b.topology_spread_constraints
        }
        _ => false,
    }
}

fn same_security_context(
    a: Option<&k8s_openapi::api::core::v1::PodSecurityContext>,
    b: Option<&k8s_openapi::api::core::v1::PodSecurityContext>,
) -> bool {
    fn effective(
        value: Option<&k8s_openapi::api::core::v1::PodSecurityContext>,
    ) -> Option<&k8s_openapi::api::core::v1::PodSecurityContext> {
        // `..Default::default()` needs the full struct literal; compare
        // against a normalized empty instead: absent and empty are the
        // same "no pod security constraints" the builder intends.
        match value {
            None => None,
            Some(context)
                if context
                    == &k8s_openapi::api::core::v1::PodSecurityContext {
                        ..Default::default()
                    } =>
            {
                None
            }
            Some(context) => Some(context),
        }
    }
    effective(a) == effective(b)
}

/// Containers agree on what the builder sets. Excluded: `terminationMessagePath`
/// and `terminationMessagePolicy` (always defaulted), plus anything else the
/// builder leaves unset. Probes, env, resources, mounts and ports are fully
/// specified by the builder, so whole-struct equality is exact there.
fn same_containers(a: &[Container], b: &[Container]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(a, b)| {
            a.name == b.name
                && a.image == b.image
                && a.image_pull_policy == b.image_pull_policy
                && a.command == b.command
                && a.args == b.args
                && a.env == b.env
                && same_container_ports(a, b)
                && a.resources == b.resources
                && a.volume_mounts == b.volume_mounts
                && a.liveness_probe == b.liveness_probe
                && a.readiness_probe == b.readiness_probe
                && a.startup_probe == b.startup_probe
        })
}

/// Container ports agree on name, port and protocol. `hostPort`/`hostIP`
/// are never set; `containerPort` is informational alongside them.
fn same_container_ports(a: &Container, b: &Container) -> bool {
    let key = |container: &Container| {
        container
            .ports
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|port| (port.name, port.container_port, port.protocol))
            .collect::<Vec<_>>()
    };
    key(a) == key(b)
}

/// Volumes agree on name plus the managed source fields. Excluded:
/// `defaultMode` on config-map and secret sources (always defaulted to
/// 420), and every source type the builder never renders.
fn same_volumes(a: Option<&[Volume]>, b: Option<&[Volume]>) -> bool {
    let key = |volumes: &[Volume]| {
        volumes
            .iter()
            .map(|volume| {
                (
                    volume.name.clone(),
                    volume
                        .config_map
                        .as_ref()
                        .map(|source| (source.name.clone(), source.items.clone(), source.optional)),
                    volume.secret.as_ref().map(|source| {
                        (
                            source.secret_name.clone(),
                            source.items.clone(),
                            source.optional,
                        )
                    }),
                    volume.empty_dir.is_some(),
                )
            })
            .collect::<Vec<_>>()
    };
    key(a.unwrap_or_default()) == key(b.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::same_statefulset;

    fn spike_cluster() -> crate::api::FlussCluster {
        let mut cluster: crate::api::FlussCluster =
            serde_yaml::from_str(include_str!("../../../../lab2/demo.yml"))
                .expect("lab2 demo CR must deserialize");
        cluster.metadata.uid = Some("test-uid".to_string());
        cluster
    }

    /// The apiserver fills defaults on write; a read-back carrying them
    /// must still compare equal, or every trigger rewrites the object.
    fn server_defaulted(
        mut sts: k8s_openapi::api::apps::v1::StatefulSet,
    ) -> k8s_openapi::api::apps::v1::StatefulSet {
        let template = sts
            .spec
            .as_mut()
            .expect("statefulset needs a spec")
            .template
            .clone();
        let mut template = template;
        let pod = template.spec.as_mut().expect("pod template needs a spec");
        pod.dns_policy = Some("ClusterFirst".to_string());
        pod.restart_policy = Some("Always".to_string());
        pod.scheduler_name = Some("default-scheduler".to_string());
        pod.service_account_name = Some("default".to_string());
        pod.termination_grace_period_seconds = Some(30);
        for volume in pod.volumes.as_mut().expect("pod needs volumes") {
            if let Some(source) = volume.config_map.as_mut() {
                source.default_mode = Some(420);
            }
            if let Some(source) = volume.secret.as_mut() {
                source.default_mode = Some(420);
            }
        }
        for container in &mut pod.containers {
            container.termination_message_path = Some("/dev/termination-log".to_string());
            container.termination_message_policy = Some("File".to_string());
            for env in container.env.as_mut().expect("container needs env") {
                if let Some(selector) = env
                    .value_from
                    .as_mut()
                    .and_then(|source| source.field_ref.as_mut())
                {
                    // The apiserver defaults fieldRef.apiVersion to v1;
                    // verified live (this exact absence rewrote
                    // StatefulSets on every trigger).
                    selector.api_version = Some("v1".to_string());
                }
            }
            for probe in [
                &mut container.liveness_probe,
                &mut container.readiness_probe,
            ]
            .into_iter()
            .flatten()
            {
                // The apiserver defaults successThreshold to 1.
                probe.success_threshold = Some(1);
            }
        }
        // The apiserver persists an empty object where the builder renders
        // nothing; verified live (this exact mismatch rewrote StatefulSets
        // on every trigger before the normalization).
        pod.security_context = Some(Default::default());
        sts.spec
            .as_mut()
            .expect("statefulset needs a spec")
            .template = template;
        // The apiserver injects claim status into stored templates; verified
        // live (this exact mismatch rewrote StatefulSets on every trigger).
        if let Some(claims) = sts
            .spec
            .as_mut()
            .expect("statefulset needs a spec")
            .volume_claim_templates
            .as_mut()
        {
            for claim in claims {
                claim.status = Some(Default::default());
            }
        }
        sts
    }

    #[test]
    fn server_defaults_do_not_read_as_drift() {
        let cluster = spike_cluster();
        for desired in [
            crate::resources::statefulset::desired_coordinator_statefulset(&cluster, None)
                .expect("valid CR must render"),
            crate::resources::statefulset::desired_tablet_statefulset(&cluster, None)
                .expect("valid CR must render"),
        ] {
            let live = server_defaulted(desired.clone());
            assert!(
                same_statefulset(&desired, &live),
                "server defaults must not read as drift"
            );
        }
    }

    #[test]
    fn real_changes_still_count() {
        let cluster = spike_cluster();
        let desired = crate::resources::statefulset::desired_tablet_statefulset(&cluster, None)
            .expect("valid CR must render");
        let mut live = server_defaulted(desired.clone());
        live.spec
            .as_mut()
            .expect("statefulset needs a spec")
            .replicas = Some(99);
        assert!(
            !same_statefulset(&desired, &live),
            "replica change must read as drift"
        );
        let mut live = server_defaulted(desired.clone());
        live.spec
            .as_mut()
            .expect("statefulset needs a spec")
            .template
            .spec
            .as_mut()
            .expect("pod template needs a spec")
            .containers[0]
            .image = Some("other:tag".to_string());
        assert!(
            !same_statefulset(&desired, &live),
            "image change must read as drift"
        );
    }

    #[test]
    fn claim_size_and_status_do_not_read_as_drift() {
        // The apiserver forbids updating claim templates on a live
        // StatefulSet, so size growth converges the live PVCs directly and
        // the template comparison must not fight the rejection. Same for
        // the injected claim status.
        let cluster = spike_cluster();
        let desired = crate::resources::statefulset::desired_tablet_statefulset(&cluster, None)
            .expect("valid CR must render");
        let mut grown = server_defaulted(desired.clone());
        grown
            .spec
            .as_mut()
            .expect("statefulset needs a spec")
            .volume_claim_templates
            .as_mut()
            .expect("tablet needs claims")[0]
            .spec
            .as_mut()
            .expect("claim needs a spec")
            .resources
            .as_mut()
            .expect("claim needs resources")
            .requests = Some(
            ["storage".to_string()]
                .into_iter()
                .map(|name| {
                    (
                        name,
                        k8s_openapi::apimachinery::pkg::api::resource::Quantity("99Gi".to_string()),
                    )
                })
                .collect(),
        );
        assert!(
            same_statefulset(&desired, &grown),
            "claim size/status must not read as drift: the apiserver forbids the update"
        );
    }

    #[test]
    fn secret_pin_rotation_is_not_drift() {
        // Rotation must report, never roll: the pin is detection-only, so a
        // changed pin alone must compare equal and leave rollout policy to
        // j5v3.
        let cluster = spike_cluster();
        let desired =
            crate::resources::statefulset::desired_tablet_statefulset(&cluster, Some("sha256:old"))
                .expect("valid CR must render");
        let rotated =
            crate::resources::statefulset::desired_tablet_statefulset(&cluster, Some("sha256:new"))
                .expect("valid CR must render");
        assert!(
            same_statefulset(&desired, &rotated),
            "pin-only change must not read as drift"
        );
    }
}
