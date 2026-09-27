// SPDX-License-Identifier: AGPL-3.0-only
//! Fluss duration strings to seconds.
//!
//! Fluss configuration uses human durations (`30s`, `5min`, `1h`); the
//! pod spec wants plain seconds. Strict subset on purpose: unknown units
//! fail closed instead of guessing.

/// Parse a Fluss duration (`500ms`, `30s`, `5min`, `1h`, `1d`) to whole
/// seconds, truncating sub-second values. Rejects empty input, missing or
/// unknown units, and non-numeric amounts.
pub fn to_seconds(raw: &str) -> Option<u64> {
    let (amount, unit) = split_unit(raw)?;
    let amount: u64 = amount.parse().ok()?;
    let factor: u64 = match unit {
        "ms" => return amount.checked_div(1_000),
        "s" => 1,
        "min" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return None,
    };
    amount.checked_mul(factor)
}

/// Split `30s` into (`30`, `s`). Longer suffixes first so `min` wins over
/// a hypothetical bare `m`; bare `m` is not a Fluss unit and is rejected.
fn split_unit(raw: &str) -> Option<(&str, &str)> {
    for suffix in ["ms", "min", "h", "d", "s"] {
        if let Some(amount) = raw.strip_suffix(suffix) {
            if !amount.is_empty() && amount.chars().all(|c| c.is_ascii_digit()) {
                return Some((amount, suffix));
            }
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::to_seconds;

    #[test]
    fn known_units_convert() {
        for (input, expected) in [
            ("30s", 30),
            ("5min", 300),
            ("1h", 3600),
            ("1d", 86400),
            ("1500ms", 1),
            ("500ms", 0),
        ] {
            assert_eq!(to_seconds(input), Some(expected), "input {input}");
        }
    }

    #[test]
    fn unknown_or_malformed_rejected() {
        for input in ["", "30", "s", "1m", "1w", "1.5h", "-5s", "5sec", " 30s"] {
            assert_eq!(to_seconds(input), None, "input {input}");
        }
    }
}
