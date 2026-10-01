// SPDX-License-Identifier: AGPL-3.0-only
//! Converge the optional, owned DNS fragment without changing cluster-wide CoreDNS.

use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;

use super::Observation;
use crate::api::FlussCluster;
use crate::controller::{Error, apply};
use crate::resources::dns_mapping;

pub async fn reconcile(
    api: &Api<ConfigMap>,
    cluster: &FlussCluster,
    uid: &str,
) -> Result<Observation, Error> {
    let name = dns_mapping::name(cluster);
    let desired = match dns_mapping::config(cluster) {
        Ok(Some(config)) => config,
        Ok(None) => {
            return match apply::ensure_absent(api, &name, uid).await {
                Ok(outcome) => Ok(Observation::ConfigMapConverged { name, outcome }),
                Err(Error::NotOwned(name)) => Ok(Observation::ConfigMapBlocked {
                    message: format!("DNS ConfigMap {name} has a different owner"),
                    name,
                }),
                Err(error) => Err(error),
            };
        }
        Err(error) => {
            return Ok(Observation::ConfigMapBlocked {
                name,
                message: error.to_string(),
            });
        }
    };
    match apply::apply(api, desired, uid, |a, b| a.data == b.data).await {
        Ok(outcome) => Ok(Observation::ConfigMapConverged { name, outcome }),
        Err(Error::NotOwned(name)) => Ok(Observation::ConfigMapBlocked {
            message: format!("DNS ConfigMap {name} has a different owner"),
            name,
        }),
        Err(error) => Err(error),
    }
}
