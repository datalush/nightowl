//! One reconciler per managed thing, coordinated by [`super::reconcile`].
//!
//! Each step converges a single resource (or the status) and reports an
//! [`Observation`]. Steps never call each other; ordering and dependencies
//! live explicitly in the coordinator. Adding a resource means a new file
//! here plus a few lines there — never a longer coordinator.

pub mod client_service;
pub mod config_map;
pub mod coordinator_service;
pub mod pod_disruption_budget;
pub mod statefulset;
pub mod status;
pub mod tablet_service;

use super::apply::ApplyOutcome;
use crate::api::ClusterHealthStatus;

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
    },
    FlussUnreachable {
        message: String,
    },
    ConfigHash {
        value: String,
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

    /// Name and message of a PodDisruptionBudget blocking progress, if any.
    pub fn blocked_pdb(&self) -> Option<(&str, &str)> {
        match self {
            Observation::PdbBlocked { name, message } => Some((name, message)),
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
