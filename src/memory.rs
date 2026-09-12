//! Memory budgeting and out-of-memory protection.
//!
//! Zentropy runs can be large by design (the canonical corpus is 10^9 bytes and
//! a full-coverage model can occupy gigabytes). A research machine that is also
//! running an editor must never have its memory exhausted by a compression run:
//! the kernel's OOM killer does not distinguish between the compressor and the
//! editor.
//!
//! This module makes every memory-heavy operation **fail closed**:
//!
//! * it computes a conservative *projected* peak for a run before allocating;
//! * it compares that projection against a budget derived from available system
//!   memory and an explicit override (`--max-ram` / `ZENTROPY_MAX_RAM_BYTES`);
//! * it refuses to start when the projection exceeds the budget, with an
//!   actionable message (use a smaller corpus rung, raise the budget, or lower
//!   the model size).
//!
//! The default budget never exceeds three quarters of currently available RAM
//! and never exceeds 8 GiB, so a run cannot drive the machine into swap or OOM
//! by mere inattention. The estimator is deliberately conservative: a false
//! refusal is cheap, an OOM kill is not.

use crate::context::ModelConfig;

/// Hard ceiling on the default budget, in bytes (8 GiB). Chosen below the
/// Hutter 10 GB envelope so a judged run is always within limits, and low
/// enough that a shared workstation keeps headroom.
pub const DEFAULT_MAX_BUDGET: u64 = 8 * 1024 * 1024 * 1024;

/// Available system memory in bytes, if it can be determined.
///
/// On Linux this reads `MemAvailable` from `/proc/meminfo`, which already
/// accounts for reclaimable page cache. Other platforms return `None`, in which
/// case only the hard ceiling applies.
pub fn available_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    let kb: u64 = rest
                        .split_whitespace()
                        .next()
                        .and_then(|x| x.parse().ok())
                        .unwrap_or(0);
                    return Some(kb.saturating_mul(1024));
                }
            }
        }
    }
    None
}

/// The memory budget for this process.
///
/// Precedence: explicit `override_bytes`, then `ZENTROPY_MAX_RAM_BYTES`, then
/// `min(3/4 * available, DEFAULT_MAX_BUDGET)`.
pub fn budget(override_bytes: Option<u64>) -> u64 {
    if let Some(b) = override_bytes {
        return b;
    }
    if let Ok(s) = std::env::var("ZENTROPY_MAX_RAM_BYTES") {
        if let Ok(v) = s.trim().parse::<u64>() {
            return v;
        }
    }
    match available_bytes() {
        Some(avail) => (avail / 4 * 3).min(DEFAULT_MAX_BUDGET),
        None => DEFAULT_MAX_BUDGET,
    }
}

/// Parse a `--max-ram` argument. Accepts plain bytes, or a suffix
/// `K`/`M`/`G`/`KiB`/`MiB`/`GiB`.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, mult) = if let Some(n) = s.strip_suffix("GiB").or_else(|| s.strip_suffix("G")) {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("MiB").or_else(|| s.strip_suffix("M")) {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix("KiB").or_else(|| s.strip_suffix("K")) {
        (n, 1024)
    } else {
        (s, 1)
    };
    num.trim()
        .parse::<u64>()
        .ok()
        .map(|v| v.saturating_mul(mult))
}

/// Bytes the context models will allocate for a coded length of `n`.
pub fn model_bytes(n: usize) -> u64 {
    ModelConfig::for_size(n as u64).memory_bytes()
}

/// Conservative projected peak for *encoding* `n` input bytes.
///
/// Components: the input buffer, a transformed buffer (hoisting can expand
/// slightly before it shrinks), an optional permutation buffer, the model's
/// output buffer (the match model needs the whole decoded history), the model
/// tables, and the archive.
pub fn projected_encode(n: u64, transforms: u64) -> u64 {
    // transforms: number of full-size intermediate buffers (0..=2).
    let input = n;
    let intermediates = transforms.min(2) * n;
    let out_buf = n; // predictor history
    let archive = n / 3 + (1 << 20);
    input + intermediates + out_buf + archive + model_bytes(n as usize)
}

/// Conservative projected peak for *decoding* an archive that produces `n`
/// bytes.
pub fn projected_decode(archive_len: u64, n: u64) -> u64 {
    let out_buf = n; // predictor history
    let decoded = n; // decoded vector
    let intermediates = n; // inverse transform / unpermute
    out_buf + decoded + intermediates + archive_len + model_bytes(n as usize)
}

/// Fail closed if a projection exceeds the budget.
pub fn check(projected: u64, budget: u64) -> Result<(), String> {
    if fits(projected, budget) {
        Ok(())
    } else {
        Err(format!(
            "projected peak {} B ({:.2} GiB) exceeds the memory budget {} B ({:.2} GiB). \
             Refusing to start to avoid an OOM kill. Use a smaller corpus rung, raise the \
             budget with --max-ram, or lower the model size.",
            projected,
            projected as f64 / (1024.0 * 1024.0 * 1024.0),
            budget,
            budget as f64 / (1024.0 * 1024.0 * 1024.0),
        ))
    }
}

/// Formatting-free budget test, for the size-constrained submission stub.
#[inline]
pub fn fits(projected: u64, budget: u64) -> bool {
    projected <= budget
}

/// A one-line human summary of the current memory situation.
pub fn summary() -> String {
    let avail = available_bytes()
        .map(|a| format!("{:.2} GiB available", a as f64 / (1024.0 * 1024.0 * 1024.0)))
        .unwrap_or_else(|| "availability unknown".to_string());
    let b = budget(None);
    format!(
        "{avail}; budget {:.2} GiB (env ZENTROPY_MAX_RAM_BYTES overrides)",
        b as f64 / (1024.0 * 1024.0 * 1024.0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sizes() {
        assert_eq!(parse_size("1024"), Some(1024));
        assert_eq!(parse_size("1K"), Some(1024));
        assert_eq!(parse_size("2M"), Some(2 * 1024 * 1024));
        assert_eq!(parse_size("3G"), Some(3 * 1024 * 1024 * 1024));
        assert_eq!(parse_size("1GiB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("nonsense"), None);
    }

    #[test]
    fn budget_is_capped_and_overridable() {
        let b = budget(Some(1234));
        assert_eq!(b, 1234);
        // Default is never above the hard ceiling.
        assert!(budget(None) <= DEFAULT_MAX_BUDGET);
    }

    #[test]
    fn projections_grow_with_n() {
        let small = projected_encode(1_000_000, 1);
        let big = projected_encode(100_000_000, 1);
        assert!(big > small);
        assert!(projected_decode(1_000_000, 100_000_000) > 100_000_000);
    }

    #[test]
    fn check_fails_closed() {
        assert!(check(100, 200).is_ok());
        let e = check(300, 200).unwrap_err();
        assert!(e.contains("exceeds the memory budget"));
    }
}
