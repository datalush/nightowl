// SPDX-License-Identifier: AGPL-3.0-only
//! Require native authentication on public listeners and a mounted admin credential.

use k8s_openapi::api::core::v1::Secret;
use kube::{Api, Client};

use crate::api::FlussCluster;
use crate::controller::{Error, reconcilers::Observation};
use crate::resources::server_config::security::CREDENTIALS_FILE;

pub async fn check(
    client: &Client,
    namespace: &str,
    cluster: &FlussCluster,
) -> Result<Vec<Observation>, Error> {
    let Some(native) = &cluster.spec.security else {
        return if cluster
            .spec
            .listeners
            .as_ref()
            .and_then(|l| l.external.as_ref())
            .is_some()
        {
            Ok(vec![Observation::ResourceBlocked {
                name: "native authentication".into(),
                message:
                    "public listener requires security.saslPlain credentials Secret and adminUser"
                        .into(),
            }])
        } else {
            Ok(vec![])
        };
    };
    let name = &native.sasl_plain.credentials_secret_name;
    let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret = match secrets.get(name).await {
        Ok(secret) => secret,
        Err(kube::Error::Api(status)) if status.code == 404 => {
            return Ok(vec![blocked(name, "missing Secret")]);
        }
        Err(error) => return Err(Error::Kube(error)),
    };
    let credentials = secret
        .data
        .as_ref()
        .and_then(|data| data.get(CREDENTIALS_FILE))
        .and_then(|bytes| std::str::from_utf8(&bytes.0).ok());
    let admin = format!("{}:", native.sasl_plain.admin_user);
    if !credentials.is_some_and(|value| {
        value.split(',').any(|entry| {
            entry
                .trim()
                .strip_prefix(&admin)
                .is_some_and(|password| !password.is_empty())
        })
    }) {
        return Ok(vec![blocked(
            name,
            "Secret credentials key must contain the configured adminUser",
        )]);
    }
    Ok(vec![])
}

fn blocked(name: &str, reason: &str) -> Observation {
    Observation::ResourceBlocked {
        name: name.into(),
        message: format!("SASL Secret {name:?}: {reason}"),
    }
}
