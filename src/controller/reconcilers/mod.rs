// SPDX-License-Identifier: AGPL-3.0-only
//! One reconciler per managed thing, coordinated by [`super::reconcile`].
//!
//! Each step converges a single resource (or the status) and reports an
//! [`Observation`]. Steps never call each other; ordering and dependencies
//! live explicitly in the coordinator. Adding a resource means a new file
//! here plus a few lines there — never a longer coordinator.

pub mod client_service;
pub mod config_map;
pub mod coordinator_service;
pub mod dns_mapping;
pub mod dynamic_config;
pub mod gateway;
pub mod network_policy;
pub mod pod_disruption_budget;
pub mod restart;
pub mod retired_endpoints;
pub mod statefulset;
pub mod status;
pub mod tablet_service;
pub mod tls_routes;
pub mod volume;

use super::apply::ApplyOutcome;
use super::fluss::TabletHealth;
use crate::api::ClusterHealthStatus;
use crate::api::RestartSeq;

/// A fact one step observed while converging.
pub enum Observation {
    ServiceConverged {
        name: String,
        outcome: ApplyOutcome,
    },
    ServiceBlocked {
        name: String,
        message: String,
    },
    ConfigMapConverged {
        name: String,
        outcome: ApplyOutcome,
    },
    ConfigMapBlocked {
        name: String,
        message: String,
    },
    StatefulSetConverged {
        name: String,
        outcome: ApplyOutcome,
    },
    StatefulSetBlocked {
        name: String,
        message: String,
    },
    /// A StatefulSet block whose cause lives outside Kubernetes and can
    /// clear with no watch event (today: the scale-in gate refusal, where
    /// Fluss emptiness changes on its own). Reported exactly like
    /// [`Observation::StatefulSetBlocked`] but retried on a timer instead
    /// of parked on `await_change`.
    StatefulSetAwaitingExternal {
        name: String,
        message: String,
    },
    WaitingForCoordinator {
        name: String,
    },
    PdbConverged {
        name: String,
        outcome: ApplyOutcome,
    },
    PdbBlocked {
        name: String,
        message: String,
    },
    FlussHealth {
        health: ClusterHealthStatus,
        coordinator_endpoints: Vec<String>,
        coordinator_ready: i32,
        tablet_uids: Vec<String>,
        tablet_health: Vec<TabletHealth>,
    },
    FlussUnreachable {
        message: String,
    },
    ConfigHash {
        value: String,
    },
    SecretFresh {
        message: String,
    },
    SecretStale {
        pods: Vec<String>,
        message: String,
    },
    VolumeBlocked {
        name: String,
        message: String,
    },
    PvcResized {
        names: Vec<String>,
    },
    StorageReady {
        evidence: Vec<String>,
    },
    StorageBlocked {
        name: String,
        message: String,
    },
    TopologyBlocked {
        message: String,
    },
    ResourceBlocked {
        name: String,
        message: String,
    },
    /// Dynamic config newly applied via Admin: the full standing map
    /// (key to value-hash) after this reconcile, replacing the field.
    DynamicConfigApplied {
        applied: std::collections::BTreeMap<String, String>,
    },
    /// Dynamic config refused or unappliable right now, with the reason.
    /// `keys` carries the rejected key names when the server refused them
    /// (restart-bound fallback); empty on transport failures, which simply
    /// retry next pass. Never rolls anything directly.
    DynamicConfigBlocked {
        message: String,
        keys: Vec<String>,
    },
    /// Sequenced restart stalled with the reason: which pod, what it waits
    /// for, which budget ran out. Steady state stays silent.
    RestartStalled {
        message: String,
    },
    /// Sequenced-restart sequence after this pass: `Some` replaces the
    /// standing run (started, advanced, or re-targeted), `None` clears a
    /// finished or stale one. Absent means carry the standing run.
    RestartSeqUpdate {
        seq: Option<RestartSeq>,
    },
    /// A keys-driven restart sequence completed: move these keys from
    /// restart-required to restart-attempted so a persistently rejected key
    /// reports instead of restart-looping.
    RestartKeysAttempted {
        keys: Vec<String>,
    },
    /// Observed running version: emitted when every pod is verified
    /// current (fresh bring-up or sequence completion). The writer prefers
    /// it over the standing value and never copies `spec.version` blindly.
    VersionObserved {
        version: String,
    },
    /// Gateway object converged: Deployment, Service or Ingress by name.
    /// `available` carries the Deployment's available replicas and is
    /// `None` for Service and Ingress observations.
    GatewayConverged {
        name: String,
        outcome: ApplyOutcome,
        available: Option<i32>,
    },
    /// Gateway refused: foreign-owned object in the way, or a referenced
    /// TLS secret missing — with the reason. Deploys nothing new.
    GatewayBlocked {
        name: String,
        message: String,
    },
    /// Gateway absent: disabled or never requested; previously managed
    /// objects garbage-collected (or already gone).
    GatewayAbsent,
    NativeRoutes {
        accepted: usize,
        desired: usize,
        gateway_programmed: bool,
    },
}

impl Observation {
    /// Name of the Service blocking progress, if any.
    pub fn blocked_service(&self) -> Option<&str> {
        match self {
            Observation::ServiceBlocked { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Name and message of a ConfigMap blocking progress, if any.
    pub fn blocked_config(&self) -> Option<(&str, &str)> {
        match self {
            Observation::ConfigMapBlocked { name, message } => Some((name, message)),
            _ => None,
        }
    }

    /// Name and message of a StatefulSet blocking progress, if any.
    pub fn blocked_statefulset(&self) -> Option<(&str, &str)> {
        match self {
            Observation::StatefulSetBlocked { name, message } => Some((name, message)),
            _ => None,
        }
    }

    /// Name and message of a StatefulSet block awaiting an external
    /// change, if any. Retried on a timer; never parked silently.
    pub fn blocked_awaiting_external(&self) -> Option<(&str, &str)> {
        match self {
            Observation::StatefulSetAwaitingExternal { name, message } => Some((name, message)),
            _ => None,
        }
    }

    /// Name and message of a PodDisruptionBudget blocking progress, if any.
    pub fn blocked_pdb(&self) -> Option<(&str, &str)> {
        match self {
            Observation::PdbBlocked { name, message } => Some((name, message)),
            _ => None,
        }
    }

    /// Name and message of a volume lifecycle block, if any.
    pub fn blocked_volume(&self) -> Option<(&str, &str)> {
        match self {
            Observation::VolumeBlocked { name, message } => Some((name, message)),
            _ => None,
        }
    }

    /// Message of a guardrail block (storage deps, topology, resources), if any.
    pub fn blocked_guardrail(&self) -> Option<&str> {
        match self {
            Observation::StorageBlocked { message, .. }
            | Observation::TopologyBlocked { message }
            | Observation::ResourceBlocked { message, .. } => Some(message),
            _ => None,
        }
    }
}
