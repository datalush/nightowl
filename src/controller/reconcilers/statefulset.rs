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
    api: &Api<StatefulSet>,
    cluster: &FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    let mut observations = Vec::with_capacity(2);
    observations.push(converge_one(api, cluster, uid, Role::Coordinator).await?);
    if coordinator_ready(api, cluster).await? {
        observations.push(converge_one(api, cluster, uid, Role::Tablet).await?);
    } else {
        let name = format!(
            "{}{}",
            cluster.metadata.name.clone().ok_or(Error::MissingName)?,
            Role::Tablet.suffix()
        );
        tracing::info!(statefulset = %name, "waiting for a ready coordinator replica");
        observations.push(Observation::WaitingForCoordinator { name });
    }
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
    ) -> Result<StatefulSet, crate::resources::server_config::ConfigError> {
        match self {
            Role::Coordinator => builder::desired_coordinator_statefulset(cluster),
            Role::Tablet => builder::desired_tablet_statefulset(cluster),
        }
    }
}

async fn converge_one(
    api: &Api<StatefulSet>,
    cluster: &FlussCluster,
    uid: &str,
    role: Role,
) -> Result<Observation, Error> {
    let name = format!(
        "{}{}",
        cluster.metadata.name.clone().ok_or(Error::MissingName)?,
        role.suffix()
    );
    let desired = match role.build(cluster) {
        Ok(desired) => desired,
        Err(e) => {
            return Ok(Observation::StatefulSetBlocked {
                name,
                message: e.to_string(),
            });
        }
    };
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
/// Only replicas, selector, pod template and claim templates participate:
/// server-defaulted fields such as `updateStrategy`, `revisionHistoryLimit`
/// or `podManagementPolicy` must never read as drift, or every read-back
/// would trigger a replace loop.
fn same_statefulset(a: &StatefulSet, b: &StatefulSet) -> bool {
    let (Some(a_spec), Some(b_spec)) = (a.spec.as_ref(), b.spec.as_ref()) else {
        return a.spec.is_none() && b.spec.is_none();
    };
    a_spec.replicas == b_spec.replicas
        && a_spec.selector == b_spec.selector
        && a_spec.template == b_spec.template
        && a_spec.volume_claim_templates == b_spec.volume_claim_templates
}
