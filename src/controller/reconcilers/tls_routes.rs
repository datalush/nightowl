// SPDX-License-Identifier: AGPL-3.0-only
//! Converge owned Gateway API SNI resources without adopting foreign routes.

use kube::api::{ApiResource, DynamicObject, GroupVersionKind};
use kube::{Api, Client};
use serde_json::Value;

use super::Observation;
use crate::api::FlussCluster;
use crate::controller::{Error, apply};
use crate::resources::tls_routes;

pub async fn reconcile(
    client: &Client,
    namespace: &str,
    cluster: &FlussCluster,
    uid: &str,
) -> Result<Vec<Observation>, Error> {
    let resources = match tls_routes::manifests(cluster) {
        Ok(value) => value,
        Err(error) => {
            return Ok(vec![Observation::GatewayBlocked {
                name: "native TLS".into(),
                message: error.to_string(),
            }]);
        }
    };
    let mut observations = Vec::new();
    let mut routes_accepted = 0;
    let mut desired_routes = 0;
    let mut gateway_programmed = false;
    for mut value in resources["items"]
        .as_array()
        .expect("generated List has items")
        .iter()
        .cloned()
    {
        let kind = value["kind"]
            .as_str()
            .expect("generated Gateway API kind")
            .to_string();
        let gvk = GroupVersionKind::gvk("gateway.networking.k8s.io", "v1", &kind);
        let resource = ApiResource::from_gvk(&gvk);
        let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &resource);
        value["metadata"]["ownerReferences"] = serde_json::json!([{
            "apiVersion": "fluss.datalush.com/v1alpha1", "kind": "FlussCluster",
            "name": cluster.metadata.name, "uid": uid,
            "controller": true, "blockOwnerDeletion": true
        }]);
        let desired: DynamicObject = serde_json::from_value(value)
            .map_err(|error| Error::InvalidConfig(format!("invalid generated {kind}: {error}")))?;
        let name = desired.metadata.name.clone().ok_or(Error::MissingName)?;
        match apply::apply(&api, desired, uid, same_spec).await {
            Ok(outcome) => observations.push(Observation::GatewayConverged {
                name: name.clone(),
                outcome,
                available: None,
            }),
            Err(Error::NotOwned(name)) => {
                observations.push(Observation::GatewayBlocked {
                    message: format!("{kind} {name} has a different owner; refusing to adopt"),
                    name,
                });
                continue;
            }
            Err(Error::Kube(kube::Error::Api(status))) if status.code == 404 => {
                observations.push(Observation::GatewayBlocked {
                    name,
                    message: format!(
                        "Gateway API {kind} is not installed; install the supported CRDs"
                    ),
                });
                continue;
            }
            Err(error) => return Err(error),
        }
        let live = api.get(&name).await.map_err(Error::Kube)?;
        if kind == "Gateway" {
            gateway_programmed = condition_true(
                &live.data["status"]["conditions"],
                "Programmed",
                live.metadata.generation,
            );
        } else {
            desired_routes += 1;
            if live.data["status"]["parents"]
                .as_array()
                .is_some_and(|parents| {
                    parents.iter().any(|parent| {
                        parent["parentRef"]["name"]
                            == cluster
                                .metadata
                                .name
                                .as_deref()
                                .map(|name| format!("{name}-native"))
                                .unwrap_or_default()
                            && condition_true(
                                &parent["conditions"],
                                "Accepted",
                                live.metadata.generation,
                            )
                            && condition_true(
                                &parent["conditions"],
                                "ResolvedRefs",
                                live.metadata.generation,
                            )
                    })
                })
            {
                routes_accepted += 1;
            }
        }
    }
    if desired_routes > 0 {
        observations.push(Observation::NativeRoutes {
            accepted: routes_accepted,
            desired: desired_routes,
            gateway_programmed,
        });
    }
    Ok(observations)
}

fn condition_true(conditions: &Value, condition: &str, generation: Option<i64>) -> bool {
    conditions.as_array().is_some_and(|conditions| {
        conditions.iter().any(|entry| {
            entry["type"] == condition
                && entry["status"] == "True"
                && entry["observedGeneration"].as_i64() == generation
        })
    })
}

fn same_spec(a: &DynamicObject, b: &DynamicObject) -> bool {
    managed_subset(&b.data["spec"], &a.data["spec"])
}

fn managed_subset(desired: &Value, live: &Value) -> bool {
    match (desired, live) {
        (Value::Object(expected), Value::Object(actual)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|existing| managed_subset(value, existing))
        }),
        (Value::Array(expected), Value::Array(actual)) => {
            expected.len() == actual.len()
                && expected
                    .iter()
                    .zip(actual)
                    .all(|(a, b)| managed_subset(a, b))
        }
        _ => desired == live,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaulted_backend_fields_do_not_cause_replacements() {
        let desired = json!({"rules": [{"backendRefs": [{"name": "tablet-0", "port": 443}]}]});
        let actual = json!({"rules": [{"backendRefs": [{"name": "tablet-0", "port": 443,
            "kind": "Service", "group": "", "weight": 1}]}]});
        assert!(managed_subset(&desired, &actual));
        assert!(!managed_subset(
            &desired,
            &json!({"rules": [{"backendRefs": [{"name": "tablet-1", "port": 443}]}]})
        ));
    }

    #[test]
    fn route_status_rejects_stale_generation_or_unprogrammed_condition() {
        assert!(condition_true(
            &json!([{"type":"Accepted","status":"True","observedGeneration":2}]),
            "Accepted",
            Some(2)
        ));
        assert!(!condition_true(
            &json!([{"type":"Accepted","status":"True","observedGeneration":1}]),
            "Accepted",
            Some(2)
        ));
        assert!(!condition_true(
            &json!([{"type":"Accepted","status":"False","observedGeneration":2}]),
            "Accepted",
            Some(2)
        ));
    }
}
