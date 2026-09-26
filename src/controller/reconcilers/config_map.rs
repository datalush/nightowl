//! Reconcile both ConfigMaps (coordinator + tablet) and report what happened.

use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;

use super::Observation;
use crate::api::FlussCluster;
use crate::constants::{COORDINATOR_CONFIG_SUFFIX, TABLET_CONFIG_SUFFIX};
use crate::controller::Error;
use crate::controller::apply;
use crate::resources::{config_map as builder, server_config::ConfigError};

/// Converge both ConfigMaps toward the desired state.
///
/// Returns one [`Observation`] per ConfigMap. Builder failures (forbidden
/// overrides, invalid values) become `ConfigMapBlocked` observations
/// instead of errors, so `.status` documents the block — mirroring the
/// Service intruder path.
pub async fn reconcile(
    api: &Api<ConfigMap>,
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
            Role::Coordinator => COORDINATOR_CONFIG_SUFFIX,
            Role::Tablet => TABLET_CONFIG_SUFFIX,
        }
    }

    fn build(self, cluster: &FlussCluster) -> Result<ConfigMap, ConfigError> {
        match self {
            Role::Coordinator => builder::desired_coordinator_config(cluster),
            Role::Tablet => builder::desired_tablet_config(cluster),
        }
    }
}

async fn converge_one(
    api: &Api<ConfigMap>,
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
            return Ok(Observation::ConfigMapBlocked {
                name,
                message: e.to_string(),
            });
        }
    };
    match apply::apply(api, desired, uid, |a, b| a.data == b.data).await {
        Ok(outcome) => Ok(Observation::ConfigMapConverged { name, outcome }),
        Err(Error::NotOwned(_)) => Ok(Observation::ConfigMapBlocked {
            message: format!(
                "configmap {name} exists with a different owner; refusing to adopt it"
            ),
            name,
        }),
        Err(e) => Err(e),
    }
}
