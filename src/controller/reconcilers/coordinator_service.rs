//! Reconcile the Coordinator headless Service and report what happened.

use k8s_openapi::api::core::v1::Service;
use kube::Api;
use kube::Resource;

use super::Observation;
use crate::controller::Error;
use crate::controller::apply;
use crate::resources::{coordinator_service as builder, service};

/// Converge the Coordinator headless Service toward the desired state.
///
/// Returns an [`Observation`] describing the outcome instead of acting on
/// it: a blocked Service is reported, not hidden, so the coordinator can
/// record it in `.status` before surfacing the error.
pub async fn reconcile(
    api: &Api<Service>,
    cluster: &crate::api::FlussCluster,
    uid: &str,
) -> Result<Observation, Error> {
    let desired = builder::desired_service(cluster);
    let name = desired.meta().name.clone().ok_or(Error::MissingName)?;

    match apply::apply(api, desired, uid, service::same_headless_service).await {
        Ok(outcome) => Ok(Observation::ServiceConverged { name, outcome }),
        Err(Error::NotOwned(svc)) => Ok(Observation::ServiceBlocked {
            message: Error::NotOwned(svc.clone()).to_string(),
            name: svc,
        }),
        Err(e) => Err(e),
    }
}
