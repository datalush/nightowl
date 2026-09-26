//! Reconcile both PodDisruptionBudgets (coordinator + tablet) and report
//! what happened.
//!
//! A disabled budget converges to absence: an owned budget is deleted, a
//! missing one is silence, and a foreign object squatting the name blocks
//! like any other intruder.

use k8s_openapi::api::policy::v1::PodDisruptionBudget;
use kube::Api;

use super::Observation;
use crate::api::FlussCluster;
use crate::constants::{COORDINATOR_PDB_SUFFIX, TABLET_PDB_SUFFIX};
use crate::controller::Error;
use crate::controller::apply;
use crate::resources::pod_disruption_budget as builder;

/// Converge both budgets toward the desired state.
pub async fn reconcile(
    api: &Api<PodDisruptionBudget>,
    cluster: &FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    let mut observations = Vec::with_capacity(2);
    observations.push(converge_one(api, cluster, uid, Role::Coordinator).await?);
    observations.push(converge_one(api, cluster, uid, Role::Tablet).await?);
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
            Role::Coordinator => COORDINATOR_PDB_SUFFIX,
            Role::Tablet => TABLET_PDB_SUFFIX,
        }
    }

    fn build(self, cluster: &FlussCluster) -> Option<PodDisruptionBudget> {
        match self {
            Role::Coordinator => builder::desired_coordinator_pdb(cluster),
            Role::Tablet => builder::desired_tablet_pdb(cluster),
        }
    }
}

async fn converge_one(
    api: &Api<PodDisruptionBudget>,
    cluster: &FlussCluster,
    uid: &str,
    role: Role,
) -> Result<Observation, Error> {
    let name = format!(
        "{}{}",
        cluster.metadata.name.clone().ok_or(Error::MissingName)?,
        role.suffix()
    );
    match role.build(cluster) {
        None => match apply::ensure_absent(api, &name, uid).await {
            Ok(outcome) => Ok(Observation::PdbConverged { name, outcome }),
            Err(Error::NotOwned(_)) => Ok(Observation::PdbBlocked {
                name: name.clone(),
                message: format!(
                    "poddisruptionbudget {name} exists with a different owner; refusing to adopt or delete it"
                ),
            }),
            Err(e) => Err(e),
        },
        Some(desired) => match apply::apply(api, desired, uid, same_pdb).await {
            Ok(outcome) => Ok(Observation::PdbConverged { name, outcome }),
            Err(Error::NotOwned(_)) => Ok(Observation::PdbBlocked {
                name: name.clone(),
                message: format!(
                    "poddisruptionbudget {name} exists with a different owner; refusing to adopt it"
                ),
            }),
            Err(e) => Err(e),
        },
    }
}

/// Budgets are the same when their disruption allowance and selector agree.
///
/// Server-defaulted fields (notably `status`) never participate, or every
/// read-back would look like drift.
fn same_pdb(a: &PodDisruptionBudget, b: &PodDisruptionBudget) -> bool {
    let (Some(a_spec), Some(b_spec)) = (a.spec.as_ref(), b.spec.as_ref()) else {
        return a.spec.is_none() && b.spec.is_none();
    };
    a_spec.min_available == b_spec.min_available
        && a_spec.max_unavailable == b_spec.max_unavailable
        && a_spec.selector == b_spec.selector
}
