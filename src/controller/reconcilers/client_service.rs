// SPDX-License-Identifier: AGPL-3.0-only
//! Reconcile the shared client Service and report what happened.
//!
//! Same contract as the headless Service steps: blocks are reported as
//! observations so `.status` documents them before the error surfaces.

use k8s_openapi::api::core::v1::Service;
use kube::Api;
use kube::Resource;

use super::Observation;
use crate::controller::Error;
use crate::controller::apply;
use crate::resources::{client_service as builder, service};

/// Converge the client Service toward the desired state.
pub async fn reconcile(
    api: &Api<Service>,
    cluster: &crate::api::FlussCluster,
    uid: &str,
) -> Result<Observation, Error> {
    let desired = builder::desired_service(cluster);
    let name = desired.meta().name.clone().ok_or(Error::MissingName)?;

    match apply::apply(api, desired, uid, service::same_client_service).await {
        Ok(outcome) => Ok(Observation::ServiceConverged { name, outcome }),
        Err(Error::NotOwned(svc)) => Ok(Observation::ServiceBlocked {
            message: Error::NotOwned(svc.clone()).to_string(),
            name: svc,
        }),
        Err(e) => Err(e),
    }
}
