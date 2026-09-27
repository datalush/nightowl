// SPDX-License-Identifier: AGPL-3.0-only
//! PVC lifecycle guard: retention is declared, expansion is earned.
//!
//! StatefulSet `volumeClaimTemplates` are immutable, so storage changes
//! cannot ride the template update. This step runs before convergence per
//! role with storage configured:
//!
//! - storage class change -> blocked (class is immutable wholesale).
//! - size shrink or unparsable size -> blocked (fail closed with the value).
//! - size growth -> allowed only when the StorageClass opts into expansion;
//!   then the live PVCs are patched in place (no pod restarts) and the
//!   template update proceeds so future pods claim the new size.
//! - fresh storage (no live claims yet) -> nothing to compare, converge freely.
//!
//! Blocks surface as observations and refuse the template update; they never
//! error hot. PVC retention itself (Retain on delete and scale) lives in the
//! rendered StatefulSet, not here.

use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use k8s_openapi::api::storage::v1::StorageClass;
use kube::Api;
use serde_json::json;

use super::Observation;
use crate::api::{FlussCluster, StorageSpec};
use crate::constants::{
    COORDINATOR_STATEFULSET_SUFFIX, LABEL_CLUSTER, LABEL_ROLE, ROLE_COORDINATOR, ROLE_TABLET,
    TABLET_STATEFULSET_SUFFIX,
};
use crate::controller::Error;
use crate::controller::guardrails::resources::parse_memory_bytes;

/// Check storage lifecycle for every role carrying storage, patching live
/// PVCs on allowed growth. Returns blocking or resized observations;
/// anything else means converge freely.
pub async fn check(
    statefulsets: &Api<StatefulSet>,
    pvcs: &Api<PersistentVolumeClaim>,
    storage_classes: &Api<StorageClass>,
    cluster: &FlussCluster,
    role: Role,
) -> Result<Option<Observation>, Error> {
    check_one(statefulsets, pvcs, storage_classes, cluster, role).await
}

#[derive(Clone, Copy)]
pub(crate) enum Role {
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

    fn label(self) -> &'static str {
        match self {
            Role::Coordinator => ROLE_COORDINATOR,
            Role::Tablet => ROLE_TABLET,
        }
    }

    fn storage(self, cluster: &FlussCluster) -> Option<&StorageSpec> {
        match self {
            Role::Coordinator => cluster.spec.coordinator.storage.as_ref(),
            Role::Tablet => Some(&cluster.spec.tablet_servers.storage),
        }
    }
}

async fn check_one(
    statefulsets: &Api<StatefulSet>,
    pvcs: &Api<PersistentVolumeClaim>,
    storage_classes: &Api<StorageClass>,
    cluster: &FlussCluster,
    role: Role,
) -> Result<Option<Observation>, Error> {
    let Some(storage) = role.storage(cluster) else {
        return Ok(None);
    };
    let name = format!(
        "{}{}",
        cluster.metadata.name.clone().ok_or(Error::MissingName)?,
        role.suffix()
    );
    let live = match statefulsets.get(&name).await {
        Ok(live) => live,
        Err(kube::Error::Api(status)) if status.code == 404 => return Ok(None),
        Err(e) => return Err(Error::Kube(e)),
    };
    let Some(old) = live_claim(&live) else {
        return Ok(None);
    };
    match decide(&old, storage) {
        StorageDecision::Unchanged => Ok(None),
        StorageDecision::ClassChanged { from, to } => Ok(Some(Observation::VolumeBlocked {
            name: name.clone(),
            message: format!(
                "statefulset {name} storage class change from {from} to {to} is forbidden: storageClassName is immutable, migrate data instead"
            ),
        })),
        StorageDecision::Shrink { from, to } => Ok(Some(Observation::VolumeBlocked {
            name: name.clone(),
            message: format!(
                "statefulset {name} storage shrink from {from} to {to} is forbidden: volumes never shrink"
            ),
        })),
        StorageDecision::Unparsable { value } => Ok(Some(Observation::VolumeBlocked {
            name: name.clone(),
            message: format!(
                "statefulset {name} has an unparsable storage size {value:?}: use plain Kubernetes quantities like 5Gi"
            ),
        })),
        StorageDecision::Grow { to, .. } => {
            grow_if_allowed(pvcs, storage_classes, cluster, role, &name, &to).await
        }
    }
}

/// Old size and class from the live claim template, if it renders one.
fn live_claim(live: &StatefulSet) -> Option<(String, Option<String>)> {
    let claim = live
        .spec
        .as_ref()?
        .volume_claim_templates
        .as_ref()?
        .iter()
        .find(|claim| claim.metadata.name.as_deref() == Some("data"))?;
    let size = claim
        .spec
        .as_ref()?
        .resources
        .as_ref()?
        .requests
        .as_ref()?
        .get("storage")?
        .0
        .clone();
    Some((size, claim.spec.as_ref()?.storage_class_name.clone()))
}

#[derive(Debug, PartialEq, Eq)]
enum StorageDecision {
    Unchanged,
    ClassChanged { from: String, to: String },
    Shrink { from: String, to: String },
    Unparsable { value: String },
    Grow { from: String, to: String },
}

/// Pure size/class comparison. Growth still needs the StorageClass to
/// allow expansion — checked live, not here.
fn decide(old: &(String, Option<String>), new: &StorageSpec) -> StorageDecision {
    let (old_size, old_class) = old;
    if old_class.as_deref() != new.storage_class_name.as_deref() {
        return StorageDecision::ClassChanged {
            from: display_class(old_class.as_deref()),
            to: display_class(new.storage_class_name.as_deref()),
        };
    }
    let (Some(old_bytes), Some(new_bytes)) =
        (parse_memory_bytes(old_size), parse_memory_bytes(&new.size))
    else {
        return StorageDecision::Unparsable {
            value: if parse_memory_bytes(old_size).is_none() {
                old_size.clone()
            } else {
                new.size.clone()
            },
        };
    };
    if new_bytes == old_bytes {
        StorageDecision::Unchanged
    } else if new_bytes < old_bytes {
        StorageDecision::Shrink {
            from: old_size.clone(),
            to: new.size.clone(),
        }
    } else {
        StorageDecision::Grow {
            from: old_size.clone(),
            to: new.size.clone(),
        }
    }
}

fn display_class(class: Option<&str>) -> String {
    match class {
        None => "(cluster default)".to_string(),
        Some(class) => format!("'{class}'"),
    }
}

/// Growth allowed only when the effective StorageClass opts into expansion:
/// the explicit class, else the cluster default. Patches every live PVC
/// carrying our labels; the template update that follows gives future pods
/// the new size. No pod restarts: expansion is online.
async fn grow_if_allowed(
    pvcs: &Api<PersistentVolumeClaim>,
    storage_classes: &Api<StorageClass>,
    cluster: &FlussCluster,
    role: Role,
    sts_name: &str,
    to: &str,
) -> Result<Option<Observation>, Error> {
    let class_name = effective_class_name(storage_classes, cluster, role).await?;
    let Some(class_name) = class_name else {
        return Ok(Some(Observation::VolumeBlocked {
            name: sts_name.to_string(),
            message: format!(
                "statefulset {sts_name} storage growth is forbidden: set an explicit storageClassName or a default StorageClass"
            ),
        }));
    };
    let allowed = match storage_classes.get(&class_name).await {
        Ok(class) => class.allow_volume_expansion.unwrap_or(false),
        Err(kube::Error::Api(status)) if status.code == 404 => false,
        Err(e) => return Err(Error::Kube(e)),
    };
    if !allowed {
        return Ok(Some(Observation::VolumeBlocked {
            name: sts_name.to_string(),
            message: format!(
                "statefulset {sts_name} storage growth is forbidden: storageClass '{class_name}' does not allow volume expansion"
            ),
        }));
    }
    let cluster_name = cluster.metadata.name.clone().ok_or(Error::MissingName)?;
    let mut resized = Vec::new();
    for pvc in pvcs.list(&Default::default()).await.map_err(Error::Kube)? {
        let labels = pvc.metadata.labels.clone().unwrap_or_default();
        if labels.get(LABEL_CLUSTER).map(String::as_str) != Some(cluster_name.as_str())
            || labels.get(LABEL_ROLE).map(String::as_str) != Some(role.label())
        {
            continue;
        }
        let current = pvc
            .spec
            .as_ref()
            .and_then(|spec| spec.resources.as_ref())
            .and_then(|resources| resources.requests.as_ref())
            .and_then(|requests| requests.get("storage"))
            .map(|quantity| quantity.0.clone())
            .unwrap_or_default();
        // Grow-only: a live claim larger than desired (from an earlier
        // growth) is left alone — patching down would destroy data, and
        // the apiserver refuses shrinks anyway.
        let grow = match (parse_memory_bytes(&current), parse_memory_bytes(to)) {
            (Some(current_bytes), Some(to_bytes)) => to_bytes > current_bytes,
            _ => current != to,
        };
        if !grow {
            continue;
        }
        let name = pvc.metadata.name.clone().ok_or(Error::MissingName)?;
        pvcs.patch(
            &name,
            &kube::api::PatchParams::default(),
            &kube::api::Patch::Merge(json!({
                "spec": { "resources": { "requests": { "storage": to } } }
            })),
        )
        .await
        .map_err(Error::Kube)?;
        resized.push(name);
    }
    if resized.is_empty() {
        return Ok(None);
    }
    Ok(Some(Observation::PvcResized { names: resized }))
}

/// Effective StorageClass: the explicit one, else the cluster default
/// (annotation `storageclass.kubernetes.io/is-default-class=true`).
async fn effective_class_name(
    storage_classes: &Api<StorageClass>,
    cluster: &FlussCluster,
    role: Role,
) -> Result<Option<String>, Error> {
    if let Some(class) = role
        .storage(cluster)
        .and_then(|storage| storage.storage_class_name.clone())
    {
        return Ok(Some(class));
    }
    let classes = storage_classes
        .list(&Default::default())
        .await
        .map_err(Error::Kube)?;
    Ok(classes
        .into_iter()
        .find(|class| {
            class
                .metadata
                .annotations
                .as_ref()
                .is_some_and(|annotations| {
                    annotations.get("storageclass.kubernetes.io/is-default-class")
                        == Some(&"true".to_string())
                })
        })
        .and_then(|class| class.metadata.name.clone()))
}

#[cfg(test)]
mod tests {
    use super::{StorageDecision, decide};
    use crate::api::StorageSpec;

    fn storage(size: &str, class: Option<&str>) -> StorageSpec {
        StorageSpec {
            size: size.to_string(),
            storage_class_name: class.map(str::to_string),
            data_dir: None,
        }
    }

    #[test]
    fn unchanged_sizes_pass_silently() {
        assert_eq!(
            decide(
                &("5Gi".to_string(), Some("fast".to_string())),
                &storage("5Gi", Some("fast")),
            ),
            StorageDecision::Unchanged,
        );
        assert_eq!(
            decide(&("5Gi".to_string(), None), &storage("5Gi", None)),
            StorageDecision::Unchanged,
        );
    }

    #[test]
    fn shrink_and_class_change_block() {
        assert_eq!(
            decide(
                &("5Gi".to_string(), Some("fast".to_string())),
                &storage("1Gi", Some("fast")),
            ),
            StorageDecision::Shrink {
                from: "5Gi".to_string(),
                to: "1Gi".to_string(),
            },
        );
        assert_eq!(
            decide(
                &("5Gi".to_string(), Some("fast".to_string())),
                &storage("5Gi", Some("slow")),
            ),
            StorageDecision::ClassChanged {
                from: "'fast'".to_string(),
                to: "'slow'".to_string(),
            },
        );
    }

    #[test]
    fn growth_needs_a_later_expansion_check() {
        assert_eq!(
            decide(&("1Gi".to_string(), None), &storage("2Gi", None)),
            StorageDecision::Grow {
                from: "1Gi".to_string(),
                to: "2Gi".to_string(),
            },
        );
    }

    #[test]
    fn unparsable_sizes_block_naming_the_value() {
        assert_eq!(
            decide(&("lots".to_string(), None), &storage("2Gi", None)),
            StorageDecision::Unparsable {
                value: "lots".to_string(),
            },
        );
    }
}
