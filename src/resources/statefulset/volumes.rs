// SPDX-License-Identifier: AGPL-3.0-only
//! Pod volumes: staged config, S3 credentials, and the data volume.
//!
//! The shared `server.yaml` lands staged (never mounted in place — the
//! image rewrites its own config at boot). The S3 Secret mounts exactly
//! where the `${directory:...}` markers point. The data volume comes from a
//! claim template when storage is configured, else a placeholder emptyDir
//! (tablets always carry storage; the coordinator only when asked).

use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, EmptyDirVolumeSource, KeyToPath, PersistentVolumeClaim,
    PersistentVolumeClaimSpec, SecretVolumeSource, Volume, VolumeMount, VolumeResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

use super::{Build, Role, STAGING_DIR};
use crate::api::{S3AuthenticationSpec, StorageSpec};
use crate::constants::{S3_ACCESS_KEY_FILE, S3_SECRET_KEY_FILE, S3_SECRETS_DIR};
use crate::resources::server_config;
use crate::resources::server_config::security;
use crate::resources::tls_proxy;

/// Volume holding the staged `server.yaml`.
const CONFIG_VOLUME: &str = "server-config";

/// Volume holding the mounted S3 Secret for secret auth. Absent under
/// workload identity, where the credential chain needs no files.
const S3_CREDENTIALS_VOLUME: &str = "s3-credentials";
const SASL_CREDENTIALS_VOLUME: &str = "sasl-credentials";

/// Data volume name, owned by the claim template when one exists.
const DATA_VOLUME: &str = "data";

impl<'a> Build<'a> {
    pub(super) fn volumes(&self) -> Vec<Volume> {
        let mut volumes = vec![Volume {
            name: CONFIG_VOLUME.to_string(),
            config_map: Some(ConfigMapVolumeSource {
                name: format!("{}{}", self.cluster_name, self.role.config_map_suffix()),
                ..Default::default()
            }),
            ..Default::default()
        }];
        if let Some(external) = self
            .cluster
            .spec
            .listeners
            .as_ref()
            .and_then(|l| l.external.as_ref())
        {
            volumes.push(Volume {
                name: tls_proxy::CONFIG_VOLUME.into(),
                config_map: Some(ConfigMapVolumeSource {
                    name: format!("{}-native-tls", self.cluster_name),
                    ..Default::default()
                }),
                ..Default::default()
            });
            volumes.push(Volume {
                name: tls_proxy::CERT_VOLUME.into(),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(external.tls.secret_name.clone()),
                    items: Some(vec![
                        KeyToPath {
                            key: "tls.crt".into(),
                            path: "tls.crt".into(),
                            ..Default::default()
                        },
                        KeyToPath {
                            key: "tls.key".into(),
                            path: "tls.key".into(),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        if let Some(native) = &self.cluster.spec.security {
            volumes.push(Volume {
                name: SASL_CREDENTIALS_VOLUME.into(),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(native.sasl_plain.credentials_secret_name.clone()),
                    items: Some(vec![KeyToPath {
                        key: security::CREDENTIALS_FILE.into(),
                        path: security::CREDENTIALS_FILE.into(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        if let S3AuthenticationSpec::Secret { secret_ref } =
            &self.cluster.spec.remote_storage.s3.authentication
        {
            // The server.yaml markers resolve against these exact file names;
            // the Secret's own key names are mapped, never required to match.
            volumes.push(Volume {
                name: S3_CREDENTIALS_VOLUME.to_string(),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(secret_ref.name.clone()),
                    items: Some(vec![
                        KeyToPath {
                            key: secret_ref.access_key_key.clone(),
                            path: S3_ACCESS_KEY_FILE.to_string(),
                            ..Default::default()
                        },
                        KeyToPath {
                            key: secret_ref.secret_key_key.clone(),
                            path: S3_SECRET_KEY_FILE.to_string(),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        // Without a claim template the data dir would have nowhere to live;
        // the emptyDir keeps the podshape valid until storage is configured.
        // Tablets always carry storage (required by the schema); the
        // coordinator only when its optional storage is set.
        if self.storage().is_none() && self.role == Role::Tablet {
            volumes.push(Volume {
                name: DATA_VOLUME.to_string(),
                empty_dir: Some(EmptyDirVolumeSource::default()),
                ..Default::default()
            });
        }
        volumes
    }

    /// Component storage when configured: required for tablets, optional
    /// for the coordinator.
    fn storage(&self) -> Option<&StorageSpec> {
        match self.role {
            Role::Coordinator => self.cluster.spec.coordinator.storage.as_ref(),
            Role::Tablet => Some(&self.cluster.spec.tablet_servers.storage),
        }
    }

    /// Persistent claim templates for the data volume, one per pod ordinal.
    ///
    /// The claim template owns the `data` volume name, so no explicit data
    /// volume is rendered alongside it. Retention and expansion policy
    /// (re82) stays out: creation only.
    pub(super) fn claims(&self) -> Option<Vec<PersistentVolumeClaim>> {
        let storage = self.storage()?;
        Some(vec![PersistentVolumeClaim {
            metadata: ObjectMeta {
                name: Some(DATA_VOLUME.to_string()),
                // Labels select our PVCs back for in-place expansion;
                // the StatefulSet controller never adopts foreign claims.
                labels: Some(self.labels.clone()),
                ..Default::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                resources: Some(VolumeResourceRequirements {
                    requests: Some(BTreeMap::from([(
                        "storage".to_string(),
                        Quantity(storage.size.clone()),
                    )])),
                    ..Default::default()
                }),
                storage_class_name: storage.storage_class_name.clone(),
                volume_mode: Some("Filesystem".to_string()),
                ..Default::default()
            }),
            status: None,
        }])
    }

    pub(super) fn mounts(&self) -> Vec<VolumeMount> {
        let mut mounts = vec![VolumeMount {
            name: CONFIG_VOLUME.to_string(),
            mount_path: STAGING_DIR.to_string(),
            ..Default::default()
        }];
        if matches!(
            self.cluster.spec.remote_storage.s3.authentication,
            S3AuthenticationSpec::Secret { .. }
        ) {
            mounts.push(VolumeMount {
                name: S3_CREDENTIALS_VOLUME.to_string(),
                mount_path: S3_SECRETS_DIR.to_string(),
                read_only: Some(true),
                ..Default::default()
            });
        }
        if self.cluster.spec.security.is_some() {
            mounts.push(VolumeMount {
                name: SASL_CREDENTIALS_VOLUME.into(),
                mount_path: security::CREDENTIALS_DIR.into(),
                read_only: Some(true),
                ..Default::default()
            });
        }
        if self.role == Role::Tablet {
            mounts.push(VolumeMount {
                name: DATA_VOLUME.to_string(),
                mount_path: server_config::storage::data_dir(self.cluster),
                ..Default::default()
            });
        }
        mounts
    }
}
