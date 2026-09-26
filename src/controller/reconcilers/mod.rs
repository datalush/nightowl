//! One reconciler per managed thing, coordinated by [`super::reconcile`].
//!
//! Each step converges a single resource (or the status) and reports an
//! [`Observation`]. Steps never call each other; ordering and dependencies
//! live explicitly in the coordinator. Adding a resource means a new file
//! here plus a few lines there — never a longer coordinator.

pub mod config_map;
pub mod coordinator_service;
pub mod status;

use super::apply::ApplyOutcome;

/// A fact one step observed while converging.
pub enum Observation {
    ServiceConverged { name: String, outcome: ApplyOutcome },
    ServiceBlocked { name: String, message: String },
    ConfigMapConverged { name: String, outcome: ApplyOutcome },
    ConfigMapBlocked { name: String, message: String },
    StorageReady { evidence: Vec<String> },
    StorageBlocked { name: String, message: String },
    TopologyBlocked { message: String },
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

    /// Message of a guardrail block (storage deps, topology), if any.
    pub fn blocked_guardrail(&self) -> Option<&str> {
        match self {
            Observation::StorageBlocked { message, .. }
            | Observation::TopologyBlocked { message } => Some(message),
            _ => None,
        }
    }
}
