use std::collections::BTreeMap;

use crate::api::{S3AuthenticationSpec, S3DelegationSpec, S3StorageSpec};

/// Render the S3-backed `server.yaml` properties.
///
/// Takes the narrowest input (`&S3StorageSpec`, not the whole cluster) so a
/// future backend dispatcher can call it without refactoring.
/// Keys rendered here that users must not override: the S3 wiring is
/// validated as a unit (location, access, credentials, delegation).
pub(crate) const PROTECTED_KEYS: &[&str] = &[
    "remote.data.dirs",
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
            "remote.data.dirs".to_string(),
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
                "${directory:/etc/fluss/secrets/s3:access-key}".to_string(),
            );
            props.insert(
                "s3.secret-key".to_string(),
                "${directory:/etc/fluss/secrets/s3:secret-key}".to_string(),
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
