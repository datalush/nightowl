// SPDX-License-Identifier: AGPL-3.0-only
use std::collections::BTreeMap;

use super::s3_profile;
use crate::api::{S3AuthenticationSpec, S3StorageSpec};
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
    // The guardrail blocks invalid combinations before workloads start. Keep
    // ConfigMap rendering fallible without a panic while it converges first.
    let resolved = s3_profile::resolve(spec).ok();
    let bucket = spec.bucket.clone();
    let prefix = spec.prefix.clone();
    let region = spec.region.clone();

    let mut props = BTreeMap::from([
        (
            "remote.data.dir".to_string(),
            format!("s3://{bucket}/{prefix}"),
        ),
        ("s3.region".to_string(), region),
        ("s3.path-style-access".to_string(), "true".to_string()),
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

    if let Some(resolved) = resolved {
        if let Some(role_arn) = resolved.role_arn {
            props.insert("s3.assumed.role.arn".to_string(), role_arn);
        }
        if let Some(endpoint) = resolved.sts_endpoint {
            props.insert("s3.assumed.role.sts.endpoint".to_string(), endpoint);
        }
    }

    props
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rustfs_preset_renders_only_file_markers_and_resolved_sts_settings() {
        let spec: S3StorageSpec = serde_yaml::from_str(
            "bucket: fluss\nprefix: clusters/lab\nregion: us-east-1\nprovider: rustfs\nendpoint: http://rustfs:9000\nauthentication:\n  type: secret\n  secretRef: {name: s3, accessKeyKey: access-key, secretKeyKey: secret-key}\n",
        )
        .expect("RustFS spec must deserialize");
        let props = properties(&spec);
        assert_eq!(props["s3.path-style-access"], "true");
        assert_eq!(props["s3.assumed.role.sts.endpoint"], "http://rustfs:9000");
        assert_eq!(
            props["s3.assumed.role.arn"],
            "arn:aws:iam::000000000000:role/fluss-clients"
        );
        assert_eq!(
            props["s3.access-key"],
            "${directory:/etc/fluss/secrets/s3:access-key}"
        );
    }
}
