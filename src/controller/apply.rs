//! Generic converge-one-object helper.
//!
//! Knows Kubernetes mechanics (get/create/replace, owner check) and nothing
//! about Fluss. Every managed resource flows through here so the ownership
//! rule is written once: never adopt an object owned by someone else.

use kube::Api;
use kube::api::PostParams;

use super::Error;
use super::guardrails::ownership::owned_by;

/// What a single [`apply`] call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Created,
    Updated,
    Unchanged,
}

/// Converge one object toward `desired`.
///
/// - Missing object -> create it.
/// - Existing object with a different controller owner -> [`Error::NotOwned`].
/// - Existing owned object where `same` is false -> replace it (keeping the
///   current `resourceVersion`, which the API requires for updates).
/// - Otherwise -> leave it alone.
///
/// `same` compares only the fields the controller manages; server-defaulted
/// fields must be ignored there or every read-back looks like drift.
pub async fn apply<K>(
    api: &Api<K>,
    mut desired: K,
    uid: &str,
    same: fn(&K, &K) -> bool,
) -> Result<ApplyOutcome, Error>
where
    K: kube::Resource + Clone + std::fmt::Debug + serde::Serialize + serde::de::DeserializeOwned,
{
    let name = desired.meta().name.clone().ok_or(Error::MissingName)?;

    match api.get(&name).await {
        Err(kube::Error::Api(status)) if status.code == 404 => {
            api.create(&PostParams::default(), &desired)
                .await
                .map_err(Error::Kube)?;
            Ok(ApplyOutcome::Created)
        }
        Err(e) => Err(Error::Kube(e)),
        Ok(existing) => {
            if !owned_by(&existing, uid) {
                return Err(Error::NotOwned(name));
            }
            if same(&existing, &desired) {
                return Ok(ApplyOutcome::Unchanged);
            }
            desired.meta_mut().resource_version = existing.meta().resource_version.clone();
            api.replace(&name, &PostParams::default(), &desired)
                .await
                .map_err(Error::Kube)?;
            Ok(ApplyOutcome::Updated)
        }
    }
}
