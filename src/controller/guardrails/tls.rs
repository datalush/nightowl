// SPDX-License-Identifier: AGPL-3.0-only
//! A TLS sidecar cannot start without a named Secret containing a keypair.

use k8s_openapi::api::core::v1::Secret;
use kube::{Api, Client};

use crate::api::FlussCluster;
use crate::controller::{Error, reconcilers::Observation};

pub async fn check(
    client: &Client,
    namespace: &str,
    cluster: &FlussCluster,
) -> Result<Vec<Observation>, Error> {
    let Some(external) = cluster
        .spec
        .listeners
        .as_ref()
        .and_then(|l| l.external.as_ref())
    else {
        return Ok(vec![]);
    };
    let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let name = &external.tls.secret_name;
    let message = match secrets.get(name).await {
        Ok(secret) => {
            let data = secret.data.as_ref();
            if ["tls.crt", "tls.key"].iter().all(|key| {
                data.and_then(|data| data.get(*key))
                    .is_some_and(|bytes| !bytes.0.is_empty())
            }) {
                return Ok(vec![]);
            }
            format!("TLS Secret {name:?} requires nonempty tls.crt and tls.key")
        }
        Err(kube::Error::Api(status)) if status.code == 404 => {
            format!("TLS Secret {name:?} is missing")
        }
        Err(error) => return Err(Error::Kube(error)),
    };
    Ok(vec![Observation::ResourceBlocked {
        name: name.clone(),
        message,
    }])
}
