// SPDX-License-Identifier: AGPL-3.0-only
//! Static heap-vs-memory backstop: `jvm.heap` must fit the container.
//!
//! A heap larger than the container memory is a guaranteed OOMKill (exit
//! 137) with no Fluss log to explain it — kernel kills look exactly like
//! server bugs. Even heap *equal* to memory is risky (Netty direct buffers
//! and RocksDB live off-heap), so the only honest posture is fail-closed:
//! refuse to converge workloads that cannot run. Pure function over the
//! spec: no API calls, no state.

use super::super::reconcilers::Observation;
use crate::api::{ComponentResourcesSpec, FlussCluster, JvmSpec};

/// Check every component's heap against its container memory.
///
/// Compares against `limits.memory` when set, else `requests.memory`.
/// Components without `jvm` are skipped (the image default applies, as does
/// whatever the container allows). Unparsable quantities block too: a value
/// the operator cannot compare is a value it must not trust.
pub fn check(cluster: &FlussCluster) -> Vec<Observation> {
    let components = [
        (
            "coordinator",
            cluster.spec.coordinator.jvm.as_ref(),
            &cluster.spec.coordinator.resources,
        ),
        (
            "tabletserver",
            cluster.spec.tablet_servers.jvm.as_ref(),
            &cluster.spec.tablet_servers.resources,
        ),
    ];
    let mut observations = Vec::new();
    for (role, jvm, resources) in components {
        if let Some(observation) = check_one(role, jvm, resources) {
            observations.push(observation);
        }
    }
    observations
}

fn check_one(
    role: &str,
    jvm: Option<&JvmSpec>,
    resources: &ComponentResourcesSpec,
) -> Option<Observation> {
    let jvm = jvm?;
    let memory = resources.limits.as_ref().unwrap_or(&resources.requests);
    match (
        parse_memory_bytes(&jvm.heap),
        parse_memory_bytes(&memory.memory),
    ) {
        (Some(heap), Some(container)) if heap > container => Some(Observation::ResourceBlocked {
            name: role.to_string(),
            message: format!(
                "{role} jvm heap {} exceeds container memory {}; lower jvm.heap or raise resources.memory",
                jvm.heap, memory.memory,
            ),
        }),
        (Some(_), Some(_)) => None,
        _ => Some(Observation::ResourceBlocked {
            name: role.to_string(),
            message: format!(
                "{role} has an unparsable memory quantity (heap {:?}, memory {:?}); use plain Kubernetes quantities like 512Mi",
                jvm.heap, memory.memory,
            ),
        }),
    }
}

/// Parse a Kubernetes memory quantity to bytes.
///
/// Supports plain bytes (`1024`), millis (`500m`), decimal SI (`k`, `M`,
/// `G`) and binary SI (`Ki`, `Mi`, `Gi`) with optional fractional digits
/// (`1.5Gi`). Larger units (`T` and up) are rejected on purpose: heaps that
/// big cannot be guarded here, so they stay forbidden alongside heap `1T`.
/// Exact integer arithmetic throughout — no floats near a block boundary.
/// Shared with the volume lifecycle guard, which compares storage sizes in
/// the same quantity language.
pub(crate) fn parse_memory_bytes(quantity: &str) -> Option<u128> {
    let split = quantity
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(quantity.len());
    let (number, suffix) = quantity.split_at(split);
    if number.is_empty() || number.chars().filter(|&c| c == '.').count() > 1 {
        return None;
    }
    let digits_only = number.chars().any(|c| c.is_ascii_digit());
    if !digits_only {
        return None;
    }
    let multiplier: u128 = match suffix {
        "" => 1,
        "m" => return divide(number, 1000),
        "k" | "K" => 1_000,
        "M" => 1_000_000,
        "G" => 1_000_000_000,
        "Ki" => 1 << 10,
        "Mi" => 1 << 20,
        "Gi" => 1 << 30,
        _ => return None,
    };
    multiply(number, multiplier)
}

/// Multiply a decimal number string by an integer, exactly.
fn multiply(number: &str, multiplier: u128) -> Option<u128> {
    let (int_part, frac_part) = match number.split_once('.') {
        Some((i, f)) => (i, f),
        None => (number, ""),
    };
    let scale = 10u128.checked_pow(frac_part.len() as u32)?;
    let int_value: u128 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    let frac_value: u128 = if frac_part.is_empty() {
        0
    } else {
        frac_part.parse().ok()?
    };
    let total = int_value.checked_mul(scale)?.checked_add(frac_value)?;
    total.checked_mul(multiplier)?.checked_div(scale)
}

/// Divide a decimal number string by an integer, exactly or not at all.
fn divide(number: &str, divisor: u128) -> Option<u128> {
    let scaled = multiply(number, 1)?;
    if scaled % divisor != 0 {
        return None;
    }
    scaled.checked_div(divisor)
}

#[cfg(test)]
mod tests {
    use super::parse_memory_bytes;
    use crate::api::{ComponentResourcesSpec, CpuMemorySpec, FlussCluster, JvmSpec};

    #[test]
    fn quantities_parse_to_bytes() {
        assert_eq!(parse_memory_bytes("1024"), Some(1024));
        assert_eq!(parse_memory_bytes("512Mi"), Some(512 * 1024 * 1024));
        assert_eq!(parse_memory_bytes("1Gi"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory_bytes("2G"), Some(2_000_000_000));
        assert_eq!(parse_memory_bytes("1k"), Some(1_000));
        assert_eq!(parse_memory_bytes("1K"), Some(1_000));
        assert_eq!(parse_memory_bytes("1.5Gi"), Some(1_610_612_736));
        assert_eq!(parse_memory_bytes("0.5G"), Some(500_000_000));
    }

    #[test]
    fn absurd_or_unknown_quantities_are_rejected() {
        for input in [
            "", "abc", "Mi", "1T", "1Ti", "1E", "500m", "-5Mi", "1.2.3Gi",
        ] {
            assert_eq!(parse_memory_bytes(input), None, "input {input}");
        }
    }

    fn cluster_with(heap: &str, memory: &str) -> FlussCluster {
        let mut cluster: FlussCluster =
            serde_yaml::from_str(include_str!("../../../../lab2/demo.yml"))
                .expect("lab2 demo CR must deserialize");
        cluster.metadata.uid = Some("test-uid".to_string());
        cluster.spec.tablet_servers.jvm = Some(JvmSpec {
            heap: heap.to_string(),
            extra_args: vec![],
        });
        cluster.spec.tablet_servers.resources = ComponentResourcesSpec {
            requests: CpuMemorySpec {
                cpu: "1".to_string(),
                memory: memory.to_string(),
            },
            limits: None,
        };
        // Silence the coordinator so only the tablet verdict shows.
        cluster.spec.coordinator.jvm = None;
        cluster
    }

    #[test]
    fn oversized_heap_blocks() {
        let observations = super::check(&cluster_with("8Gi", "1Gi"));
        assert_eq!(observations.len(), 1, "one block, tablet only");
    }

    #[test]
    fn fitting_heap_passes_silently() {
        let observations = super::check(&cluster_with("1Gi", "2Gi"));
        assert!(observations.is_empty());
    }
}
