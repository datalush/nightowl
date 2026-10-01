// SPDX-License-Identifier: AGPL-3.0-only
//! Reconcile both ConfigMaps (coordinator + tablet) and report what happened.

use k8s_openapi::api::core::v1::ConfigMap;
use kube::Api;

use super::Observation;
use crate::api::FlussCluster;
use crate::constants::{
    CONFIG_DATA_KEY, CONFIG_HASH_ANNOTATION, COORDINATOR_CONFIG_SUFFIX, TABLET_CONFIG_SUFFIX,
};
use crate::controller::Error;
use crate::controller::apply;
use crate::resources::{config_map as builder, server_config::ConfigError};
use crate::utils::hash;

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
    let mut observations = Vec::with_capacity(3);
    let (coord_obs, coord_yaml) = converge_one(api, cluster, uid, Role::Coordinator).await?;
    let (tablet_obs, tablet_yaml) = converge_one(api, cluster, uid, Role::Tablet).await?;
    observations.push(coord_obs);
    observations.push(tablet_obs);
    if let (Some(coordinator_yaml), Some(tablet_yaml)) = (coord_yaml, tablet_yaml) {
        let coordinator_yaml =
            crate::resources::external_access::rollout_input(cluster, true, coordinator_yaml);
        let tablet_yaml =
            crate::resources::external_access::rollout_input(cluster, false, tablet_yaml);
        observations.push(Observation::ConfigHash {
            value: hash::combined_config_hash(&coordinator_yaml, &tablet_yaml),
        });
    }
    Ok(observations)
}

/// The sidecar's static Envoy configuration is distinct from server.yaml.
pub async fn reconcile_tls(
    api: &Api<ConfigMap>,
    cluster: &FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    let Some(desired) = crate::resources::tls_proxy::desired_config_map(cluster) else {
        return Ok(vec![]);
    };
    let name = desired.metadata.name.clone().ok_or(Error::MissingName)?;
    let observation = match apply::apply(api, desired, uid, |a, b| a.data == b.data).await {
        Ok(outcome) => Observation::ConfigMapConverged { name, outcome },
        Err(Error::NotOwned(name)) => Observation::ConfigMapBlocked {
            message: format!("configmap {name} exists with a different owner"),
            name,
        },
        Err(error) => return Err(error),
    };
    Ok(vec![observation])
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
) -> Result<(Observation, Option<String>), Error> {
    let name = format!(
        "{}{}",
        cluster.metadata.name.clone().ok_or(Error::MissingName)?,
        role.suffix()
    );
    let desired = match role.build(cluster) {
        Ok(desired) => desired,
        Err(e) => {
            return Ok((
                Observation::ConfigMapBlocked {
                    name,
                    message: e.to_string(),
                },
                None,
            ));
        }
    };
    let server_yaml = desired
        .data
        .as_ref()
        .and_then(|data| data.get(CONFIG_DATA_KEY))
        .cloned();
    match apply::apply(api, desired, uid, same_config).await {
        Ok(outcome) => Ok((
            Observation::ConfigMapConverged { name, outcome },
            server_yaml,
        )),
        Err(Error::NotOwned(_)) => Ok((
            Observation::ConfigMapBlocked {
                message: format!(
                    "configmap {name} exists with a different owner; refusing to adopt it"
                ),
                name,
            },
            server_yaml,
        )),
        Err(e) => Err(e),
    }
}

/// ConfigMaps are the same when their rendered content and our content-hash
/// annotation agree.
///
/// Only our own annotation participates: server-added annotations such as
/// `kubectl.kubernetes.io/last-applied-configuration` must never read as
/// drift, or every external touch would trigger a replace loop.
fn same_config(a: &ConfigMap, b: &ConfigMap) -> bool {
    a.data == b.data && config_hash_annotation(a) == config_hash_annotation(b)
}

fn config_hash_annotation(cm: &ConfigMap) -> Option<&String> {
    cm.metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(CONFIG_HASH_ANNOTATION))
}
