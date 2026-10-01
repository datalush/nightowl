// SPDX-License-Identifier: AGPL-3.0-only
//! Converge the policy before public sidecars or Fluss workloads start.

use k8s_openapi::api::networking::v1::NetworkPolicy;
use kube::Api;

use super::Observation;
use crate::api::FlussCluster;
use crate::controller::{Error, apply};

pub async fn reconcile(
    api: &Api<NetworkPolicy>,
    cluster: &FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    let Some(desired) = crate::resources::network_policy::desired_policy(cluster) else {
        return Ok(vec![]);
    };
    let name = desired.metadata.name.clone().ok_or(Error::MissingName)?;
    match apply::apply(api, desired, uid, |a, b| a.spec == b.spec).await {
        Ok(outcome) => Ok(vec![Observation::GatewayConverged {
            name,
            outcome,
            available: None,
        }]),
        Err(Error::NotOwned(name)) => Ok(vec![Observation::GatewayBlocked {
            message: format!("NetworkPolicy {name} has a different owner"),
            name,
        }]),
        Err(error) => Err(error),
    }
}
