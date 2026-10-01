// SPDX-License-Identifier: AGPL-3.0-only
//! Remote-storage reference preflight (security surface).
//!
//! Renders nothing; verifies the objects `server.yaml` will point at
//! actually exist: the referenced Secret with the referenced keys, or the
//! referenced ServiceAccount. Findings flow into the `RemoteStorageReady`
//! condition; kube API errors other than 404 propagate as transient
//! failures and keep the retry path.

use k8s_openapi::api::core::v1::{Secret, ServiceAccount};
use kube::{Api, Client};

use super::super::reconcilers::Observation;
use crate::api::{FlussCluster, S3AuthenticationSpec, S3SecretRef};
use crate::controller::Error;
use crate::resources::server_config::storage::backends::s3_profile;

/// Check the S3 references of a cluster against live cluster state.
///
/// Always returns exactly one [`Observation`]: `StorageReady` with presence
/// evidence when everything resolves, `StorageBlocked` naming the missing
/// object or keys otherwise.
pub async fn check(
    client: &Client,
    namespace: &str,
    cluster: &FlussCluster,
) -> Result<Vec<Observation>, Error> {
    if let Err(message) = s3_profile::resolve(&cluster.spec.remote_storage.s3) {
        return Ok(vec![Observation::StorageBlocked {
            name: "s3 delegation".into(),
            message,
        }]);
    }
    match &cluster.spec.remote_storage.s3.authentication {
        S3AuthenticationSpec::Secret { secret_ref } => {
            check_secret(client, namespace, secret_ref).await
        }
        S3AuthenticationSpec::WorkloadIdentity {
            service_account_name,
        } => check_service_account(client, namespace, service_account_name).await,
    }
    .map(|observation| vec![observation])
}

/// The referenced Secret must exist and carry both credential keys.
async fn check_secret(
    client: &Client,
    namespace: &str,
    secret_ref: &S3SecretRef,
) -> Result<Observation, Error> {
    let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
    let secret = match secrets.get(&secret_ref.name).await {
        Ok(secret) => secret,
        Err(kube::Error::Api(status)) if status.code == 404 => {
            return Ok(Observation::StorageBlocked {
                name: secret_ref.name.clone(),
                message: format!(
                    "secret '{}' referenced by s3 secretRef not found in namespace '{}'",
                    secret_ref.name, namespace
                ),
            });
        }
        Err(e) => return Err(Error::Kube(e)),
    };
    Ok(check_secret_keys(&secret, secret_ref))
}

/// Both credential keys must be present in the Secret data.
fn check_secret_keys(secret: &Secret, secret_ref: &S3SecretRef) -> Observation {
    let data = secret.data.clone().unwrap_or_default();
    let missing: Vec<String> = [&secret_ref.access_key_key, &secret_ref.secret_key_key]
        .into_iter()
        .filter(|key| !data.contains_key(key.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() {
        Observation::StorageReady {
            evidence: vec![format!(
                "secret '{}' present with keys {}",
                secret_ref.name,
                [
                    secret_ref.access_key_key.clone(),
                    secret_ref.secret_key_key.clone()
                ]
                .join(", ")
            )],
        }
    } else {
        Observation::StorageBlocked {
            name: secret_ref.name.clone(),
            message: format!(
                "secret '{}' is missing keys: {}",
                secret_ref.name,
                missing.join(", ")
            ),
        }
    }
}

/// The referenced ServiceAccount must exist; the credential chain itself
/// (IRSA, EKS Pod Identity) lives outside what the operator can observe.
async fn check_service_account(
    client: &Client,
    namespace: &str,
    service_account_name: &str,
) -> Result<Observation, Error> {
    let accounts: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
    match accounts.get(service_account_name).await {
        Ok(_) => Ok(Observation::StorageReady {
            evidence: vec![format!("serviceaccount '{service_account_name}' present")],
        }),
        Err(kube::Error::Api(status)) if status.code == 404 => Ok(Observation::StorageBlocked {
            name: service_account_name.to_string(),
            message: format!(
                "serviceaccount '{service_account_name}' referenced by s3 workloadIdentity not found in namespace '{namespace}'"
            ),
        }),
        Err(e) => Err(Error::Kube(e)),
    }
}
