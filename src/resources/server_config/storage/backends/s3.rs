// SPDX-License-Identifier: AGPL-3.0-only
use std::collections::BTreeMap;

use crate::api::{S3AuthenticationSpec, S3DelegationSpec, S3StorageSpec};
use crate::constants::{S3_ACCESS_KEY_FILE, S3_SECRET_KEY_FILE, S3_SECRETS_DIR};

/// Render the S3-backed `server.yaml` properties.
///
/// The single location renders into the singular `remote.data.dir`: the
/// plural `remote.data.dirs` is documented for newer Fluss, but the 1.0.0
/// image ignores it and fails startup on a null remote path (verified live:
/// `Can not create a Path from a null string`). See ADR-0001 amendment.
pub(crate) const PROTECTED_KEYS: &[&str] = &[
    "remote.data.dir",
    "s3.region",
    "s3.endpoint",
    "s3.path-style-access",
    "config.providers",
    "config.providers.directory.param.allowed.paths",
    "s3.access-key",
    "s3.secret-key",
    "s3.assumed.role.arn",
    "s3.assumed.role.sts.endpoint",
];

pub(crate) fn properties(spec: &S3StorageSpec) -> BTreeMap<String, String> {
    let bucket = spec.bucket.clone();
    let prefix = spec.prefix.clone();
    let region = spec.region.clone();

    let mut props = BTreeMap::from([
        (
            "remote.data.dir".to_string(),
            format!("s3://{bucket}/{prefix}"),
        ),
        ("s3.region".to_string(), region),
        (
            "s3.path-style-access".to_string(),
            spec.path_style_access.to_string(),
        ),
    ]);

    if let Some(endpoint) = spec.endpoint.clone() {
        props.insert("s3.endpoint".to_string(), endpoint);
    }

    match &spec.authentication {
        S3AuthenticationSpec::Secret { .. } => {
            props.insert("config.providers".to_string(), "directory".to_string());
            props.insert(
                "config.providers.directory.param.allowed.paths".to_string(),
                "/etc/fluss/secrets".to_string(),
            );
            props.insert(
                "s3.access-key".to_string(),
                format!("${{directory:{S3_SECRETS_DIR}:{S3_ACCESS_KEY_FILE}}}"),
            );
            props.insert(
                "s3.secret-key".to_string(),
                format!("${{directory:{S3_SECRETS_DIR}:{S3_SECRET_KEY_FILE}}}"),
            );
        }
        S3AuthenticationSpec::WorkloadIdentity { .. } => {}
    }

    if let Some(S3DelegationSpec::AssumeRole {
        role_arn,
        sts_endpoint,
    }) = spec.delegation.as_ref()
    {
        props.insert("s3.assumed.role.arn".to_string(), role_arn.clone());
        if let Some(endpoint) = sts_endpoint.clone() {
            props.insert("s3.assumed.role.sts.endpoint".to_string(), endpoint);
        }
    }

    props
}
