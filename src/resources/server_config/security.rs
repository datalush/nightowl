// SPDX-License-Identifier: AGPL-3.0-only
//! Native SASL authentication and ACLs with credentials from a mounted Secret.

use std::collections::BTreeMap;

use crate::api::FlussCluster;
use crate::resources::server_config::ConfigError;

pub const CREDENTIALS_DIR: &str = "/etc/fluss/secrets/sasl";
pub const CREDENTIALS_FILE: &str = "credentials";
const OWNED_KEYS: &[&str] = &[
    "security.protocol.map",
    "security.sasl.enabled.mechanisms",
    "security.sasl.plain.credentials",
    "authorizer.enabled",
    "super.users",
];

pub fn properties(cluster: &FlussCluster) -> Result<BTreeMap<String, String>, ConfigError> {
    let Some(security) = &cluster.spec.security else {
        return Ok(BTreeMap::new());
    };
    let spec = &security.sasl_plain;
    if spec.credentials_secret_name.trim().is_empty()
        || spec.admin_user.is_empty()
        || !spec
            .admin_user
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err(ConfigError::InvalidValue {
            key: "security.saslPlain".into(),
            value: "(invalid)".into(),
            reason: "credentialsSecretName and an ASCII alphanumeric adminUser are required".into(),
        });
    }
    for key in cluster
        .spec
        .configuration_overrides
        .keys()
        .chain(cluster.spec.coordinator.configuration_overrides.keys())
        .chain(cluster.spec.tablet_servers.configuration_overrides.keys())
    {
        if OWNED_KEYS.contains(&key.as_str()) {
            return Err(ConfigError::InvalidValue {
                key: key.clone(),
                value: "(overridden)".into(),
                reason: "native security controls this key; remove the conflicting override".into(),
            });
        }
    }
    let listeners = cluster.spec.resolved_listeners();
    let mut map = format!(
        "{}:PLAINTEXT,{}:SASL",
        listeners.internal.name, listeners.client.name
    );
    if let Some(external) = &listeners.external {
        map.push_str(&format!(",{}:SASL", external.name));
    }
    Ok(BTreeMap::from([
        ("config.providers".into(), "directory".into()),
        (
            "config.providers.directory.param.allowed.paths".into(),
            "/etc/fluss/secrets".into(),
        ),
        ("security.protocol.map".into(), map),
        ("security.sasl.enabled.mechanisms".into(), "PLAIN".into()),
        (
            "security.sasl.plain.credentials".into(),
            format!("${{directory:{CREDENTIALS_DIR}:{CREDENTIALS_FILE}}}"),
        ),
        ("authorizer.enabled".into(), "true".into()),
        ("super.users".into(), format!("User:{}", spec.admin_user)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{NativeSecuritySpec, SaslPlainSpec};

    #[test]
    fn native_acl_uses_secret_marker_and_protects_authorizer_settings() {
        let mut cluster = crate::resources::external_access::tests::cluster();
        cluster.spec.security = Some(NativeSecuritySpec {
            sasl_plain: SaslPlainSpec {
                credentials_secret_name: "native-sasl".into(),
                admin_user: "admin".into(),
            },
        });
        let props = properties(&cluster).unwrap();
        assert_eq!(
            props["security.protocol.map"],
            "INTERNAL:PLAINTEXT,CLIENT:SASL,EXTERNAL:SASL"
        );
        assert_eq!(
            props["security.sasl.plain.credentials"],
            "${directory:/etc/fluss/secrets/sasl:credentials}"
        );
        assert_eq!(props["super.users"], "User:admin");
        cluster
            .spec
            .configuration_overrides
            .insert("authorizer.enabled".into(), "false".into());
        assert!(properties(&cluster).is_err());
    }
}
