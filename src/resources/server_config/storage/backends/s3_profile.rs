// SPDX-License-Identifier: AGPL-3.0-only
//! Resolve S3 backend presets; path-style and AssumeRole are the common case.

use crate::api::{S3AuthenticationSpec, S3DelegationType, S3Provider, S3StorageSpec};

// RustFS 1.0 accepts RoleArn for AWS compatibility but derives permissions
// from the signing IAM user's policies; this name is not an AWS IAM role.
const RUSTFS_ROLE_ARN: &str = "arn:aws:iam::000000000000:role/fluss-clients";

pub struct ResolvedS3 {
    pub role_arn: Option<String>,
    pub sts_endpoint: Option<String>,
}

pub fn resolve(spec: &S3StorageSpec) -> Result<ResolvedS3, String> {
    let provider = spec.provider.unwrap_or(S3Provider::Aws);
    if provider == S3Provider::Rustfs
        && spec
            .endpoint
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
    {
        return Err("provider rustfs requires the S3 endpoint".into());
    }

    let delegation = spec.delegation.as_ref();
    let role = delegation.and_then(|value| value.role_arn.as_ref());
    let kind = delegation
        .and_then(|value| value.kind)
        .unwrap_or(S3DelegationType::AssumeRole);
    let role_arn = match kind {
        S3DelegationType::AssumeRole => {
            let role = role
                .map(String::as_str)
                .or_else(|| (provider == S3Provider::Rustfs).then_some(RUSTFS_ROLE_ARN));
            let Some(role) = role.filter(|value| !value.trim().is_empty()) else {
                return Err("AssumeRole requires delegation.roleArn".into());
            };
            Some(role.to_owned())
        }
        S3DelegationType::GetSessionToken => {
            if provider == S3Provider::Rustfs {
                return Err("RustFS supports AssumeRole, not GetSessionToken".into());
            }
            if role.is_some() {
                return Err("getSessionToken cannot set delegation.roleArn".into());
            }
            None
        }
    };
    if matches!(
        spec.authentication,
        S3AuthenticationSpec::WorkloadIdentity { .. }
    ) {
        if provider == S3Provider::Rustfs {
            return Err("provider rustfs requires secret authentication (IAM user keys)".into());
        }
        if role_arn.is_none() {
            return Err("workloadIdentity requires delegation.roleArn".into());
        }
    }
    let sts_endpoint = delegation
        .and_then(|value| value.sts_endpoint.clone())
        .or_else(|| {
            (provider == S3Provider::Rustfs)
                .then(|| spec.endpoint.clone())
                .flatten()
        });
    if sts_endpoint
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err("delegation.stsEndpoint must not be blank".into());
    }

    Ok(ResolvedS3 {
        role_arn,
        sts_endpoint,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(yaml: &str) -> S3StorageSpec {
        serde_yaml::from_str(yaml).expect("valid S3 spec")
    }

    const BASE: &str = "bucket: fluss\nprefix: cluster/a\nregion: us-east-1\nauthentication:\n  type: secret\n  secretRef: {name: s3, accessKeyKey: access-key, secretKeyKey: secret-key}\n";

    #[test]
    fn omitted_provider_matches_explicit_aws_and_requires_role() {
        assert!(resolve(&config(BASE)).is_err());
        assert!(resolve(&config(&format!("{BASE}provider: aws\n"))).is_err());
        let with_role =
            format!("{BASE}delegation:\n  roleArn: arn:aws:iam::123456789012:role/reader\n");
        let implicit = config(&with_role);
        let explicit = config(&format!("{with_role}provider: aws\n"));
        assert_eq!(
            resolve(&implicit).expect("implicit AWS role").role_arn,
            resolve(&explicit).expect("explicit AWS role").role_arn
        );
    }

    #[test]
    fn rustfs_preset_and_explicit_overrides_are_independent() {
        let rustfs = config(&format!(
            "{BASE}provider: rustfs\nendpoint: http://rustfs:9000\n"
        ));
        let resolved = resolve(&rustfs).expect("RustFS preset resolves");
        assert_eq!(resolved.sts_endpoint.as_deref(), Some("http://rustfs:9000"));
        assert_eq!(resolved.role_arn.as_deref(), Some(RUSTFS_ROLE_ARN));

        let custom = config(&format!(
            "{BASE}provider: rustfs\nendpoint: http://rustfs:9000\ndelegation:\n  roleArn: arn:aws:iam::123456789012:role/reader\n  stsEndpoint: https://sts.example.test\n"
        ));
        let resolved = resolve(&custom).expect("overrides resolve");
        assert_eq!(
            resolved.role_arn.as_deref(),
            Some("arn:aws:iam::123456789012:role/reader")
        );
        assert_eq!(
            resolved.sts_endpoint.as_deref(),
            Some("https://sts.example.test")
        );
    }

    #[test]
    fn custom_s3_endpoint_does_not_guess_a_provider() {
        let spec = config(&format!(
            "{BASE}endpoint: https://objects.example.test\ndelegation:\n  roleArn: arn:aws:iam::123456789012:role/reader\n  stsEndpoint: https://sts.example.test\n"
        ));
        let resolved = resolve(&spec).expect("custom endpoint resolves");
        assert_eq!(
            resolved.sts_endpoint.as_deref(),
            Some("https://sts.example.test")
        );
        assert!(resolved.role_arn.is_some());
        let session = config(&format!(
            "{BASE}endpoint: https://objects.example.test\ndelegation:\n  type: getSessionToken\n  stsEndpoint: https://sts.example.test\n"
        ));
        let resolved = resolve(&session).expect("custom GetSessionToken resolves");
        assert!(resolved.role_arn.is_none());
        assert_eq!(
            resolved.sts_endpoint.as_deref(),
            Some("https://sts.example.test")
        );
    }

    #[test]
    fn invalid_delegation_is_blocked_before_workloads() {
        for yaml in [
            format!("{BASE}provider: rustfs\n"),
            format!("{BASE}delegation:\n  type: assumeRole\n"),
            format!("{BASE}delegation:\n  type: getSessionToken\n  roleArn: wrong\n"),
            format!(
                "{BASE}provider: rustfs\nendpoint: http://rustfs:9000\ndelegation:\n  type: getSessionToken\n"
            ),
            format!(
                "{BASE}provider: rustfs\nendpoint: http://rustfs:9000\ndelegation:\n  stsEndpoint: ' '\n"
            ),
        ] {
            assert!(
                resolve(&config(&yaml)).is_err(),
                "expected rejection: {yaml}"
            );
        }
        let workload = "bucket: fluss\nprefix: cluster/a\nregion: us-east-1\nprovider: aws\nauthentication:\n  type: workloadIdentity\n  serviceAccountName: fluss\n";
        assert!(resolve(&config(workload)).is_err());
    }
}
