// SPDX-License-Identifier: AGPL-3.0-only
//! Sequenced restarts (j5v3): one pod at a time, health-gated, explained.
//!
//! The StatefulSets run OnDelete, so template updates (config hash, image)
//! never roll pods on their own: every restart is a sequenced, health-gated
//! pod delete from this step. Tablets go tail-first, then coordinators.
//!
//! Completion memory lives in `.status.restart_seq`: which ordinals already
//! restarted toward the target hash (or for restart-bound keys). Everything
//! else is live state — pod hash annotations for staleness, Ready
//! transitions for stabilization — so an operator restart mid-sequence
//! resumes instead of duplicating deletions. At most one delete per pass,
//! and never while another pod is still verifying.
//!
//! Safety rules, all fail-closed with `RestartStalled` evidence:
//! - No fresh GREEN health this pass means hands off (silence, like the
//!   dynamic-config gate).
//! - A pod missing, terminating, unready, or freshly ready makes the step
//!   wait; exceeding the recovery budget or an unparsable timeout stalls.
//! - Stabilization is measured off the pod's own Ready transition time,
//!   which survives operator restarts.

use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use kube::api::DeleteParams;

use super::Observation;
use crate::api::{ClusterHealthState, FlussCluster, RestartSeq};
use crate::constants::{
    CONFIG_HASH_ANNOTATION, COORDINATOR_STATEFULSET_SUFFIX, TABLET_STATEFULSET_SUFFIX,
};
use crate::controller::Error;
use crate::utils::duration;

/// Recovery budget when `rollingUpgrade` is absent: a replacement pod gets
/// five minutes to appear Ready before the sequence stalls.
const DEFAULT_RECOVERY_SECS: u64 = 300;

/// Stabilization wait when `rollingUpgrade` is absent: one full health
/// cycle past a pod's Ready transition before the next deletion.
const DEFAULT_STABILIZATION_SECS: u64 = 60;

/// What a live pod looks like to the sequencer. Built at the IO boundary;
/// every decision below is pure over these views.
#[derive(Clone, Debug, PartialEq)]
struct PodView {
    exists: bool,
    terminating_for_secs: Option<i64>,
    ready: bool,
    /// Seconds since the Ready transition; `None` when never Ready.
    ready_for_secs: Option<i64>,
    /// Seconds since creation; `None` when the pod is absent.
    age_secs: Option<i64>,
    /// Whether the pod booted with the desired config hash.
    hash_matches: bool,
}

/// One pass decision: at most one deletion, else wait, stall, or silence.
#[derive(Clone, Debug, PartialEq)]
enum Action {
    Delete(i32),
    Wait,
    Stalled(String),
    Idle,
}

/// Inputs for one role's pass, bundled to keep functions narrow.
struct PlanInputs<'a> {
    done: &'a [i32],
    views: &'a [(i32, PodView)],
    keys_run: bool,
    recovery_secs: u64,
    stabilization_secs: u64,
}

/// Plan one role's pass over tail-first ordinals. Wait-states apply to
/// every ordinal — done or not — so a replacement that never becomes Ready
/// stalls instead of being skipped over. Returns ordinals newly verified
/// plus the pass action.
fn plan_role(inputs: &PlanInputs) -> (Vec<i32>, Action) {
    let mut newly_done = Vec::new();
    let mut ordinals: Vec<i32> = inputs.views.iter().map(|(o, _)| *o).collect();
    ordinals.sort_by(|a, b| b.cmp(a));
    for ordinal in ordinals {
        let view = inputs
            .views
            .iter()
            .find(|(o, _)| *o == ordinal)
            .map(|(_, view)| view)
            .expect("ordinal comes from the views");
        let name = format!("ts-{ordinal}");
        if !view.exists {
            return (newly_done, Action::Wait);
        }
        if let Some(terminating_for) = view.terminating_for_secs {
            if terminating_for > inputs.recovery_secs as i64 {
                return (
                    newly_done,
                    Action::Stalled(format!(
                        "{name} stuck terminating beyond the {}s recovery budget",
                        inputs.recovery_secs
                    )),
                );
            }
            return (newly_done, Action::Wait);
        }
        if !view.ready {
            let age = view.age_secs.unwrap_or(0);
            if age > inputs.recovery_secs as i64 {
                return (
                    newly_done,
                    Action::Stalled(format!(
                        "{name} replacement not Ready within the {}s recovery budget",
                        inputs.recovery_secs
                    )),
                );
            }
            return (newly_done, Action::Wait);
        }
        if view.ready_for_secs.unwrap_or(0) < inputs.stabilization_secs as i64 {
            return (newly_done, Action::Wait);
        }
        if inputs.done.contains(&ordinal) {
            continue;
        }
        if inputs.keys_run || !view.hash_matches {
            return (newly_done, Action::Delete(ordinal));
        }
        newly_done.push(ordinal);
    }
    (newly_done, Action::Idle)
}

/// Actual pod object name for an ordinal.
fn pod_object_name(cluster: &FlussCluster, coordinator: bool, ordinal: i32) -> Option<String> {
    let base = cluster.metadata.name.clone()?;
    let suffix = if coordinator {
        COORDINATOR_STATEFULSET_SUFFIX
    } else {
        TABLET_STATEFULSET_SUFFIX
    };
    Some(format!("{base}{suffix}-{ordinal}"))
}

/// Read one pod into a view; missing (404) reads as absent, anything else
/// failing aborts the pass transiently.
async fn read_pod(
    pods: &Api<Pod>,
    name: &str,
    desired_hash: &str,
    now_secs: i64,
) -> Result<PodView, Error> {
    let pod = match pods.get(name).await {
        Ok(pod) => pod,
        Err(kube::Error::Api(status)) if status.code == 404 => {
            return Ok(PodView {
                exists: false,
                terminating_for_secs: None,
                ready: false,
                ready_for_secs: None,
                age_secs: None,
                hash_matches: false,
            });
        }
        Err(e) => return Err(Error::Kube(e)),
    };
    let meta = pod.metadata;
    let terminating_for_secs = meta
        .deletion_timestamp
        .as_ref()
        .map(|time| now_secs - time.0.as_second());
    let age_secs = meta
        .creation_timestamp
        .as_ref()
        .map(|time| now_secs - time.0.as_second());
    let hash_matches = meta
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get(CONFIG_HASH_ANNOTATION))
        .is_some_and(|hash| hash == desired_hash);
    let (ready, ready_for_secs) = pod
        .status
        .as_ref()
        .and_then(|status| status.conditions.as_ref())
        .and_then(|conditions| {
            conditions.iter().find(|c| c.type_ == "Ready").map(|c| {
                (
                    c.status == "True",
                    c.last_transition_time
                        .as_ref()
                        .map(|time| now_secs - time.0.as_second()),
                )
            })
        })
        .unwrap_or((false, None));
    Ok(PodView {
        exists: true,
        terminating_for_secs,
        ready,
        ready_for_secs,
        age_secs,
        hash_matches,
    })
}

/// Parse the rolling-upgrade budgets, or fail closed with the reason.
fn budgets(cluster: &FlussCluster) -> Result<(u64, u64), String> {
    let (recovery_raw, stabilization_raw) = match cluster.spec.rolling_upgrade.as_ref() {
        None => return Ok((DEFAULT_RECOVERY_SECS, DEFAULT_STABILIZATION_SECS)),
        Some(upgrade) => (
            upgrade.recovery_timeout.as_str(),
            upgrade.stabilization_window.as_str(),
        ),
    };
    let recovery = duration::to_seconds(recovery_raw).ok_or_else(|| {
        format!("unparsable rollingUpgrade.recoveryTimeout {recovery_raw:?}: failing closed")
    })?;
    let stabilization = duration::to_seconds(stabilization_raw).ok_or_else(|| {
        format!(
            "unparsable rollingUpgrade.stabilizationWindow {stabilization_raw:?}: failing closed"
        )
    })?;
    Ok((recovery, stabilization))
}

/// Fresh GREEN health gates every deletion; without it the step stays
/// silent (the health conditions already explain an unhealthy cluster).
fn fresh_green(observations: &[Observation]) -> bool {
    observations.iter().any(|observation| match observation {
        Observation::FlussHealth { health, .. } => {
            matches!(health.status, ClusterHealthState::Green)
        }
        _ => false,
    })
}

/// Desired combined config hash from this pass, or `None` when the render
/// is blocked (the config-map step already reported that).
fn desired_hash(observations: &[Observation]) -> Option<String> {
    observations
        .iter()
        .find_map(|observation| match observation {
            Observation::ConfigHash { value } => Some(value.clone()),
            _ => None,
        })
}

/// Read all pods of one role into ordinal views.
async fn read_role(
    pods: &Api<Pod>,
    cluster: &FlussCluster,
    coordinator: bool,
    replicas: i32,
    desired: &str,
    now_secs: i64,
) -> Result<Vec<(i32, PodView)>, Error> {
    let mut views = Vec::new();
    for ordinal in 0..replicas {
        let Some(name) = pod_object_name(cluster, coordinator, ordinal) else {
            return Err(Error::MissingName);
        };
        views.push((ordinal, read_pod(pods, &name, desired, now_secs).await?));
    }
    Ok(views)
}

/// Sequence a restart pass for one cluster.
///
/// Returns observations for the status writer; empty means idle — nothing
/// due, or nothing safe to do right now. Both read as silence, not churn.
pub async fn reconcile(
    pods: &Api<Pod>,
    cluster: &FlussCluster,
    observations: &[Observation],
) -> Result<Vec<Observation>, Error> {
    let standing_seq = cluster.status.as_ref().and_then(|s| s.restart_seq.clone());
    let standing_required: Vec<String> = cluster
        .status
        .as_ref()
        .map(|s| s.restart_required_keys.clone())
        .unwrap_or_default();
    let Some(desired) = desired_hash(observations) else {
        return Ok(Vec::new());
    };
    let (recovery_secs, stabilization_secs) = match budgets(cluster) {
        Ok(budgets) => budgets,
        Err(message) => {
            return Ok(vec![Observation::RestartStalled { message }]);
        }
    };
    if !fresh_green(observations) {
        return Ok(Vec::new());
    }
    let now_secs = k8s_openapi::jiff::Timestamp::now().as_second();
    // A stale run (config moved again mid-sequence) restarts over from a
    // fresh target; a run with nothing left due clears below.
    let mut seq = match standing_seq {
        Some(seq) if seq.target_hash == desired => seq,
        _ => RestartSeq {
            target_hash: desired.clone(),
            for_keys: false,
            done_tablet_ordinals: Vec::new(),
            done_coordinator_ordinals: Vec::new(),
        },
    };
    if !standing_required.is_empty()
        && seq.done_tablet_ordinals.is_empty()
        && seq.done_coordinator_ordinals.is_empty()
    {
        seq.for_keys = true;
    }
    let tablet_replicas = cluster.spec.tablet_servers.replicas;
    let tablet_views = read_role(pods, cluster, false, tablet_replicas, &desired, now_secs).await?;
    let tablet_inputs = PlanInputs {
        done: &seq.done_tablet_ordinals,
        views: &tablet_views,
        keys_run: seq.for_keys,
        recovery_secs,
        stabilization_secs,
    };
    let (newly_done, action) = plan_role(&tablet_inputs);
    seq.done_tablet_ordinals.extend(newly_done.iter().cloned());
    seq.done_tablet_ordinals.sort_unstable();
    seq.done_tablet_ordinals.dedup();
    match action {
        Action::Delete(ordinal) => {
            let name = pod_object_name(cluster, false, ordinal).expect("ordinal was read");
            pods.delete(&name, &DeleteParams::default())
                .await
                .map_err(Error::Kube)?;
            return Ok(vec![Observation::RestartSeqUpdate { seq: Some(seq) }]);
        }
        Action::Wait => {
            return Ok(vec![Observation::RestartSeqUpdate { seq: Some(seq) }]);
        }
        Action::Stalled(message) => {
            return Ok(vec![
                Observation::RestartSeqUpdate { seq: Some(seq) },
                Observation::RestartStalled { message },
            ]);
        }
        Action::Idle => {}
    }
    // Tablets verified: same dance for the coordinators, tail-first. With
    // one coordinator the leader check is vacuous beyond GREEN; the shape
    // stays generic for N replicas.
    let coordinator_replicas = cluster.spec.coordinator.replicas;
    let coordinator_views = read_role(
        pods,
        cluster,
        true,
        coordinator_replicas,
        &desired,
        now_secs,
    )
    .await?;
    let coordinator_inputs = PlanInputs {
        done: &seq.done_coordinator_ordinals,
        views: &coordinator_views,
        keys_run: seq.for_keys,
        recovery_secs,
        stabilization_secs,
    };
    let (newly_done, action) = plan_role(&coordinator_inputs);
    seq.done_coordinator_ordinals
        .extend(newly_done.iter().cloned());
    seq.done_coordinator_ordinals.sort_unstable();
    seq.done_coordinator_ordinals.dedup();
    match action {
        Action::Delete(ordinal) => {
            let name = pod_object_name(cluster, true, ordinal).expect("ordinal was read");
            pods.delete(&name, &DeleteParams::default())
                .await
                .map_err(Error::Kube)?;
            return Ok(vec![Observation::RestartSeqUpdate { seq: Some(seq) }]);
        }
        Action::Wait => {
            return Ok(vec![Observation::RestartSeqUpdate { seq: Some(seq) }]);
        }
        Action::Stalled(message) => {
            return Ok(vec![
                Observation::RestartSeqUpdate { seq: Some(seq) },
                Observation::RestartStalled { message },
            ]);
        }
        Action::Idle => {}
    }
    // Everything verified: clear the run; a keys-driven run additionally
    // moves its keys to attempted so persistent rejection reports instead
    // of restart-looping.
    let mut out = vec![Observation::RestartSeqUpdate { seq: None }];
    if seq.for_keys {
        out.push(Observation::RestartKeysAttempted {
            keys: standing_required,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Action, PlanInputs, PodView, budgets, plan_role};

    const RECOVERY: u64 = 300;
    const STABILIZATION: u64 = 60;

    fn current_baked() -> PodView {
        PodView {
            exists: true,
            terminating_for_secs: None,
            ready: true,
            ready_for_secs: Some(600),
            age_secs: Some(3600),
            hash_matches: true,
        }
    }

    fn stale_baked() -> PodView {
        PodView {
            hash_matches: false,
            ..current_baked()
        }
    }

    fn inputs<'a>(views: &'a [(i32, PodView)], done: &'a [i32], keys_run: bool) -> PlanInputs<'a> {
        PlanInputs {
            done,
            views,
            keys_run,
            recovery_secs: RECOVERY,
            stabilization_secs: STABILIZATION,
        }
    }

    #[test]
    fn stale_tail_deletes_first() {
        let views = vec![(0, stale_baked()), (1, current_baked()), (2, stale_baked())];
        let (done, action) = plan_role(&inputs(&views, &[], false));
        assert_eq!(action, Action::Delete(2));
        assert!(done.is_empty(), "a delete advances nothing yet");
    }

    #[test]
    fn current_pods_verify_without_deletes() {
        let views = vec![(0, current_baked()), (1, current_baked())];
        let (done, action) = plan_role(&inputs(&views, &[], false));
        assert_eq!(action, Action::Idle);
        assert_eq!(done, vec![1, 0], "tail-first verification order");
    }

    #[test]
    fn keys_run_deletes_matching_pods_once() {
        let views = vec![(0, current_baked()), (1, current_baked())];
        let (_, action) = plan_role(&inputs(&views, &[], true));
        assert_eq!(action, Action::Delete(1));
        let (_, action) = plan_role(&inputs(&views, &[1], true));
        assert_eq!(action, Action::Delete(0));
        let (done, action) = plan_role(&inputs(&views, &[0, 1], true));
        assert_eq!(action, Action::Idle);
        assert_eq!(done.len(), 0, "all done already, nothing new");
    }

    #[test]
    fn missing_or_baking_pods_wait() {
        let missing = PodView {
            exists: false,
            ..current_baked()
        };
        let views = vec![(0, current_baked()), (1, missing)];
        let (_, action) = plan_role(&inputs(&views, &[], false));
        assert_eq!(action, Action::Wait);

        let young = PodView {
            ready_for_secs: Some(5),
            ..current_baked()
        };
        let views = vec![(0, young)];
        let (_, action) = plan_role(&inputs(&views, &[], false));
        assert_eq!(action, Action::Wait);
    }

    #[test]
    fn recovery_budget_exceeded_stalls() {
        let stuck = PodView {
            ready: false,
            ready_for_secs: None,
            age_secs: Some(900),
            ..current_baked()
        };
        let views = vec![(0, stuck)];
        let (_, action) = plan_role(&inputs(&views, &[], false));
        assert!(
            matches!(action, Action::Stalled(_)),
            "old unready pod must stall, got: {action:?}"
        );

        let terminating = PodView {
            terminating_for_secs: Some(900),
            ..current_baked()
        };
        let views = vec![(0, terminating)];
        let (_, action) = plan_role(&inputs(&views, &[], false));
        assert!(
            matches!(action, Action::Stalled(_)),
            "stuck termination must stall, got: {action:?}"
        );
    }

    #[test]
    fn budgets_default_and_reject_garbage() {
        let mut cluster: crate::api::FlussCluster =
            serde_yaml::from_str(include_str!("../../../../lab2/demo.yml"))
                .expect("lab2 demo CR must deserialize");
        assert_eq!(
            budgets(&cluster),
            Ok((300, 60)),
            "absent rollingUpgrade means documented defaults"
        );
        cluster.spec.rolling_upgrade = Some(crate::api::RollingUpgradeSpec {
            controlled_shutdown_timeout: "2min".to_string(),
            recovery_timeout: "soon".to_string(),
            stabilization_window: "1min".to_string(),
        });
        assert!(
            budgets(&cluster).is_err(),
            "unparsable budgets fail closed instead of guessing"
        );
    }
}
