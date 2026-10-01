// SPDX-License-Identifier: AGPL-3.0-only
//! Publish configured mappings only when their owned Services converged this pass.

use crate::api::{ExternalEndpointStatus, FlussCluster};
use crate::controller::reconcilers::Observation;

pub(super) fn endpoints(
    cluster: &FlussCluster,
    observations: &[Observation],
) -> Vec<ExternalEndpointStatus> {
    crate::resources::external_access::endpoints(cluster)
        .unwrap_or_default()
        .into_iter()
        .filter(|endpoint| {
            observations.iter().any(|observation| matches!(
            observation, Observation::ServiceConverged { name, .. } if name == &endpoint.service
        ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_addresses_are_not_published_without_converged_backends() {
        let mut cluster = crate::resources::external_access::tests::cluster();
        let stale = crate::resources::external_access::endpoints(&cluster).unwrap();
        cluster.status = Some(crate::api::FlussClusterStatus {
            external_endpoints: stale.clone(),
            ..Default::default()
        });
        assert!(endpoints(&cluster, &[]).is_empty());
        assert!(
            endpoints(
                &cluster,
                &[Observation::ServiceBlocked {
                    name: stale[0].service.clone(),
                    message: "foreign owner".into(),
                }]
            )
            .is_empty()
        );
    }
}
