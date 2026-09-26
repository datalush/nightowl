//! One reconciler per managed thing, coordinated by [`super::reconcile`].
//!
//! Each step converges a single resource (or the status) and reports an
//! [`Observation`]. Steps never call each other; ordering and dependencies
//! live explicitly in the coordinator. Adding a resource means a new file
//! here plus a few lines there — never a longer coordinator.

pub mod coordinator_service;
pub mod status;

use super::apply::ApplyOutcome;

/// A fact one step observed while converging.
pub enum Observation {
    ServiceConverged { name: String, outcome: ApplyOutcome },
    ServiceBlocked { name: String, message: String },
}

impl Observation {
    /// Name of the Service blocking progress, if any.
    pub fn blocked_service(&self) -> Option<&str> {
        match self {
            Observation::ServiceBlocked { name, .. } => Some(name),
            Observation::ServiceConverged { .. } => None,
        }
    }
}
