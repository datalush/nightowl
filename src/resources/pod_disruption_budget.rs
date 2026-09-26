//! Desired PodDisruptionBudgets, one per role at most.
//!
//! Tablets get the FIP-41 safe default (`maxUnavailable: 0`) unless the CR
//! says otherwise; the Coordinator only gets a budget when the CR asks for
//! one. An explicit `enabled: false` means no budget — the reconciler
//! removes an owned one instead of converging it. Direct pod deletes bypass
//! any PDB; the budgets only gate eviction-mediated drains.

use std::collections::BTreeMap;

use k8s_openapi::api::policy::v1::{PodDisruptionBudget, PodDisruptionBudgetSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{LabelSelector, ObjectMeta, OwnerReference};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

use crate::api::FlussCluster;
use crate::constants::{
    API_VERSION, COORDINATOR_PDB_SUFFIX, DEFAULT_TABLET_MAX_UNAVAILABLE, KIND_FLUSS_CLUSTER,
    LABEL_CLUSTER, LABEL_ROLE, ROLE_COORDINATOR, ROLE_TABLET, TABLET_PDB_SUFFIX,
};

/// Desired TabletServer budget, or `None` when the CR disables it.
///
/// Absent `podDisruptionBudget` means the safe default, not no budget.
pub fn desired_tablet_pdb(cluster: &FlussCluster) -> Option<PodDisruptionBudget> {
    let name = cluster_name(cluster);
    match cluster.spec.pod_disruption_budget.as_ref() {
        None => Some(build_pdb(
            cluster,
            &name,
            ROLE_TABLET,
            TABLET_PDB_SUFFIX,
            None,
            Some(DEFAULT_TABLET_MAX_UNAVAILABLE),
        )),
        Some(spec) if !spec.tablet_servers.enabled => None,
        Some(spec) => Some(build_pdb(
            cluster,
            &name,
            ROLE_TABLET,
            TABLET_PDB_SUFFIX,
            None,
            Some(spec.tablet_servers.max_unavailable),
        )),
    }
}

/// Desired Coordinator budget, or `None` unless the CR enables one.
///
/// Unlike tablets there is no safe default here: a single coordinator with
/// `minAvailable: 1` blocks every eviction including harmless ones, so
/// silence means no budget.
pub fn desired_coordinator_pdb(cluster: &FlussCluster) -> Option<PodDisruptionBudget> {
    let name = cluster_name(cluster);
    let spec = cluster.spec.pod_disruption_budget.as_ref()?;
    let coordinator = spec.coordinator.as_ref()?;
    if !coordinator.enabled {
        return None;
    }
    Some(build_pdb(
        cluster,
        &name,
        ROLE_COORDINATOR,
        COORDINATOR_PDB_SUFFIX,
        Some(coordinator.min_available),
        None,
    ))
}

fn build_pdb(
    cluster: &FlussCluster,
    cluster_name: &str,
    role: &str,
    suffix: &str,
    min_available: Option<i32>,
    max_unavailable: Option<i32>,
) -> PodDisruptionBudget {
    let labels = BTreeMap::from([
        (LABEL_CLUSTER.to_string(), cluster_name.to_string()),
        (LABEL_ROLE.to_string(), role.to_string()),
    ]);
    let uid = cluster
        .metadata
        .uid
        .clone()
        .expect("FlussCluster needs a uid");
    PodDisruptionBudget {
        metadata: ObjectMeta {
            name: Some(format!("{cluster_name}{suffix}")),
            namespace: cluster.metadata.namespace.clone(),
            labels: Some(labels.clone()),
            owner_references: Some(vec![OwnerReference {
                api_version: API_VERSION.to_string(),
                kind: KIND_FLUSS_CLUSTER.to_string(),
                name: cluster_name.to_string(),
                uid,
                controller: Some(true),
                block_owner_deletion: Some(true),
            }]),
            ..Default::default()
        },
        spec: Some(PodDisruptionBudgetSpec {
            min_available: min_available.map(IntOrString::Int),
            max_unavailable: max_unavailable.map(IntOrString::Int),
            selector: Some(LabelSelector {
                match_labels: Some(labels),
                ..Default::default()
            }),
            ..Default::default()
        }),
        status: None,
    }
}

fn cluster_name(cluster: &FlussCluster) -> String {
    cluster
        .metadata
        .name
        .clone()
        .expect("FlussCluster needs a name")
}

#[cfg(test)]
mod tests {
    use super::{desired_coordinator_pdb, desired_tablet_pdb};
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

    fn spike_cluster() -> crate::api::FlussCluster {
        let mut cluster: crate::api::FlussCluster =
            serde_yaml::from_str(include_str!("../../../lab2/demo.yml"))
                .expect("lab2 demo CR must deserialize");
        cluster.metadata.uid = Some("test-uid".to_string());
        cluster
    }

    #[test]
    fn tablet_defaults_to_zero_max_unavailable() {
        let pdb = desired_tablet_pdb(&spike_cluster()).expect("tablets get a default budget");
        let spec = pdb.spec.as_ref().expect("budget needs a spec");
        assert_eq!(spec.max_unavailable, Some(IntOrString::Int(0)));
        assert_eq!(spec.min_available, None);
        let selector = spec.selector.as_ref().expect("budget needs a selector");
        assert_eq!(
            selector
                .match_labels
                .as_ref()
                .expect("selector needs labels")["fluss.datalush.com/role"],
            "tabletserver",
        );
    }

    #[test]
    fn disabled_budgets_render_nothing() {
        let mut cluster = spike_cluster();
        cluster.spec.pod_disruption_budget = Some(crate::api::PodDisruptionBudgetSpec {
            tablet_servers: crate::api::TabletDisruptionBudgetSpec {
                enabled: false,
                max_unavailable: 1,
            },
            coordinator: None,
        });
        assert!(
            desired_tablet_pdb(&cluster).is_none(),
            "explicit disable removes the budget"
        );
        assert!(
            desired_coordinator_pdb(&cluster).is_none(),
            "coordinator defaults to no budget"
        );
    }

    #[test]
    fn coordinator_budget_is_opt_in() {
        let mut cluster = spike_cluster();
        cluster.spec.pod_disruption_budget = Some(crate::api::PodDisruptionBudgetSpec {
            tablet_servers: crate::api::TabletDisruptionBudgetSpec {
                enabled: true,
                max_unavailable: 0,
            },
            coordinator: Some(crate::api::CoordinatorDisruptionBudgetSpec {
                enabled: true,
                min_available: 1,
            }),
        });
        let pdb = desired_coordinator_pdb(&cluster).expect("opt-in coordinator budget");
        let spec = pdb.spec.as_ref().expect("budget needs a spec");
        assert_eq!(spec.min_available, Some(IntOrString::Int(1)));
        assert_eq!(spec.max_unavailable, None);
    }
}
