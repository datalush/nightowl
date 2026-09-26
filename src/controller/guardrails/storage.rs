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
use crate::api::{FlussCluster, S3AuthenticationSpec};
use crate::controller::Error;

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
    let s3 = &cluster.spec.remote_storage.s3;
    match &s3.authentication {
        S3AuthenticationSpec::Secret { secret_ref } => {
            let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
            match secrets.get(&secret_ref.name).await {
                Err(kube::Error::Api(status)) if status.code == 404 => {
                    Ok(vec![Observation::StorageBlocked {
                        name: secret_ref.name.clone(),
                        message: format!(
                            "secret '{}' referenced by s3 secretRef not found in namespace '{}'",
                            secret_ref.name, namespace
                        ),
                    }])
                }
                Err(e) => Err(Error::Kube(e)),
                Ok(secret) => {
                    let data = secret.data.unwrap_or_default();
                    let mut missing = Vec::new();
                    for key in [&secret_ref.access_key_key, &secret_ref.secret_key_key] {
                        if !data.contains_key(key.as_str()) {
                            missing.push(key.clone());
                        }
                    }
                    if missing.is_empty() {
                        Ok(vec![Observation::StorageReady {
                            evidence: vec![format!(
                                "secret '{}' present with keys {}",
                                secret_ref.name,
                                [
                                    secret_ref.access_key_key.clone(),
                                    secret_ref.secret_key_key.clone()
                                ]
                                .join(", ")
                            )],
                        }])
                    } else {
                        Ok(vec![Observation::StorageBlocked {
                            name: secret_ref.name.clone(),
                            message: format!(
                                "secret '{}' is missing keys: {}",
                                secret_ref.name,
                                missing.join(", ")
                            ),
                        }])
                    }
                }
            }
        }
        S3AuthenticationSpec::WorkloadIdentity {
            service_account_name,
        } => {
            let accounts: Api<ServiceAccount> = Api::namespaced(client.clone(), namespace);
            match accounts.get(service_account_name).await {
                Err(kube::Error::Api(status)) if status.code == 404 => {
                    Ok(vec![Observation::StorageBlocked {
                        name: service_account_name.clone(),
                        message: format!(
                            "serviceaccount '{service_account_name}' referenced by s3 workloadIdentity not found in namespace '{namespace}'"
                        ),
                    }])
                }
                Err(e) => Err(Error::Kube(e)),
                Ok(_) => Ok(vec![Observation::StorageReady {
                    evidence: vec![format!("serviceaccount '{service_account_name}' present")],
                }]),
            }
        }
    }
}
