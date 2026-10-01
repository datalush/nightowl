// SPDX-License-Identifier: AGPL-3.0-only
//! Gateway API conditions, never a fabricated claim of external reachability.

use super::super::Observation;
use super::common;
use crate::api::{ConditionStatus, FlussCluster, FlussClusterCondition, FlussConditionType};

pub(super) fn condition(
    cluster: &FlussCluster,
    observations: &[Observation],
) -> Option<FlussClusterCondition> {
    cluster.spec.listeners.as_ref()?.external.as_ref()?;
    let result = observations
        .iter()
        .find_map(|observation| match observation {
            Observation::NativeRoutes {
                accepted,
                desired,
                gateway_programmed,
            } => Some((*accepted, *desired, *gateway_programmed)),
            _ => None,
        });
    let (status, reason, message) = match result {
        Some((accepted, desired, true)) if accepted == desired => (
            ConditionStatus::True,
            "GatewayProgrammed",
            format!("Gateway programmed; {accepted}/{desired} SNI routes accepted and resolved"),
        ),
        Some((accepted, desired, _)) => (
            ConditionStatus::Unknown,
            "GatewayPending",
            format!("Gateway or SNI routes pending: {accepted}/{desired} accepted"),
        ),
        None => (
            ConditionStatus::False,
            "GatewayBlocked",
            "Gateway resources could not be reconciled".to_string(),
        ),
    };
    Some(common::condition(
        cluster,
        FlussConditionType::NativeRoutesProgrammed,
        status,
        reason.to_string(),
        message,
        Vec::new(),
    ))
}
