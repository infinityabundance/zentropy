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
//!
//! Three layers, because a startup check alone is not protection:
//!
//! 1. **Startup, judged-safe** ([`budget`]) — `min(3/4 * available, 8 GiB)`. The
//!    submission stub uses this: a judged run owns its machine, so it must not
//!    reserve headroom it is entitled to.
//! 2. **Startup, workstation-safe** ([`research_budget`]) — the same, minus
//!    [`RESERVE_BYTES`]; the research driver shares the machine with an editor,
//!    so it refuses to claim the reserve.
//! 3. **Runtime floor** ([`enforce_runtime_floor`]) — a long run re-checks
//!    available memory as it codes and aborts if the machine tightens underneath
//!    it. This is what stops a two-hour job from swapping a user's session to
//!    death. It is **opt-in** ([`enable_runtime_guard`]) so the judged decoder
//!    can never abort a legitimate reconstruction.
//!
//! A hard kernel-enforced ceiling (`ulimit -v`) is applied by the long-run
//! wrappers in `tools/`; see `tools/p9_gate.sh`.

use std::sync::atomic::{AtomicBool, Ordering};

/// Hard ceiling on the default budget, in bytes (8 GiB). Chosen below the
/// Hutter 10 GB envelope so a judged run is always within limits, and low
/// enough that a shared workstation keeps headroom.
pub const DEFAULT_MAX_BUDGET: u64 = 8 * 1024 * 1024 * 1024;

/// Headroom the research plane leaves for the rest of the workstation.
///
/// A research run shares the machine with an editor, a browser and a desktop, so
/// unlike a judged run it must not claim everything that is available. Four GiB
/// is enough for a heavy editor session; below this the run refuses to start.
pub const RESERVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Available memory below which a *running* research job aborts.
///
/// The distinction from [`RESERVE_BYTES`] matters: the reserve stops a run from
/// *starting* when the machine is already tight, while this floor stops a run
/// that was fine at startup from thrashing the machine an hour later.
pub const RUN_FLOOR_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Bytes coded between runtime memory checks. Cheap enough to be free (one
/// relaxed atomic load per megabyte) and frequent enough to react in seconds.
pub const RUNTIME_CHECK_INTERVAL: usize = 1 << 20;

/// Whether the runtime floor is armed. Off by default: the submission stub must
/// never abort a legitimate reconstruction because the judge's machine is busy.
static RUNTIME_GUARD: AtomicBool = AtomicBool::new(false);

/// Arm the runtime memory floor. The research driver calls this once at startup;
/// the submission stub never does.
pub fn enable_runtime_guard() {
    RUNTIME_GUARD.store(true, Ordering::Relaxed);
}

/// The running floor, honouring `ZENTROPY_RUN_FLOOR_BYTES`.
pub fn run_floor() -> u64 {
    std::env::var("ZENTROPY_RUN_FLOOR_BYTES")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(RUN_FLOOR_BYTES)
}

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
///
/// This is the **judged-safe** policy: a submission run owns its machine, so it
/// does not reserve headroom. The research driver uses [`research_budget`].
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

/// The memory budget for a **research** run: identical to [`budget`] except that
/// it never claims [`RESERVE_BYTES`], so a shared workstation keeps its headroom.
///
/// When available memory has fallen to the reserve, the budget is zero and every
/// memory-heavy command refuses to start — the intended fail-closed behaviour.
pub fn research_budget(override_bytes: Option<u64>) -> u64 {
    if let Some(b) = override_bytes {
        return b;
    }
    if let Ok(s) = std::env::var("ZENTROPY_MAX_RAM_BYTES") {
        if let Ok(v) = s.trim().parse::<u64>() {
            return v;
        }
    }
    match available_bytes() {
        Some(avail) => avail
            .saturating_sub(RESERVE_BYTES)
            .min(avail / 4 * 3)
            .min(DEFAULT_MAX_BUDGET),
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

/// Bytes the context models will allocate for a coded length of `n`. Uses the
/// **accepted** configuration so the projection includes every adopted
/// mechanism (state experts, the SSE stage, the PPM model).
pub fn model_bytes(n: usize) -> u64 {
    crate::archive::ACCEPTED_METHOD.config_for(n).memory_bytes()
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

/// True while the machine still has the running floor available.
pub fn runtime_ok() -> bool {
    match available_bytes() {
        Some(a) => a >= run_floor(),
        None => true,
    }
}

/// Called from the coding loops every [`RUNTIME_CHECK_INTERVAL`] bytes.
///
/// The guard is a no-op unless [`enable_runtime_guard`] was called, so it costs
/// one relaxed atomic load and a predictable branch per megabyte on the research
/// path and nothing on the judged path. When it does fire it **aborts**: the
/// point is to stop a long run before the kernel's OOM killer starts choosing
/// victims, and the victim is otherwise likely to be the user's editor rather
/// than the compressor.
#[inline]
pub fn enforce_runtime_floor() {
    if !RUNTIME_GUARD.load(Ordering::Relaxed) {
        return;
    }
    let floor = run_floor();
    if let Some(a) = available_bytes() {
        if a < floor {
            panic!(
                "memory floor breached: {:.2} GiB available, floor {:.2} GiB. \
                 Aborting this run to protect the machine. Free memory, or lower the \
                 model with --max-ram / ZENTROPY_MAX_RAM_BYTES, then retry.",
                a as f64 / (1024.0 * 1024.0 * 1024.0),
                floor as f64 / (1024.0 * 1024.0 * 1024.0),
            );
        }
    }
}

/// A one-line human summary of the current memory situation.
pub fn summary() -> String {
    let gib = 1024.0 * 1024.0 * 1024.0;
    let avail = available_bytes()
        .map(|a| format!("{:.2} GiB available", a as f64 / gib))
        .unwrap_or_else(|| "availability unknown".to_string());
    format!(
        "{avail}; research budget {:.2} GiB, judged budget {:.2} GiB, write reserve {:.2} GiB, \
         run floor {:.2} GiB (env ZENTROPY_MAX_RAM_BYTES / ZENTROPY_RUN_FLOOR_BYTES override)",
        research_budget(None) as f64 / gib,
        budget(None) as f64 / gib,
        RESERVE_BYTES as f64 / gib,
        run_floor() as f64 / gib,
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
        assert!(research_budget(None) <= DEFAULT_MAX_BUDGET);
        // The research budget is never *more* than the judged budget: it gives
        // the rest of the workstation its reserve.
        assert!(research_budget(None) <= budget(None));
    }

    #[test]
    fn research_budget_keeps_the_reserve() {
        // With plenty available both policies hit the ceiling...
        let plenty = DEFAULT_MAX_BUDGET * 4 + RESERVE_BYTES;
        let judged = (plenty / 4 * 3).min(DEFAULT_MAX_BUDGET);
        let research = plenty
            .saturating_sub(RESERVE_BYTES)
            .min(plenty / 4 * 3)
            .min(DEFAULT_MAX_BUDGET);
        assert_eq!(judged, DEFAULT_MAX_BUDGET);
        assert_eq!(research, DEFAULT_MAX_BUDGET);
        // ...but when the machine is tight the reserve is the difference.
        let tight = RESERVE_BYTES + 2 * 1024 * 1024 * 1024;
        let judged = (tight / 4 * 3).min(DEFAULT_MAX_BUDGET);
        let research = tight
            .saturating_sub(RESERVE_BYTES)
            .min(tight / 4 * 3)
            .min(DEFAULT_MAX_BUDGET);
        assert!(research < judged, "research={research} judged={judged}");
        assert_eq!(research, 2 * 1024 * 1024 * 1024);
        // Below the reserve the budget is zero: fail closed, do not start.
        let none = (RESERVE_BYTES / 2)
            .saturating_sub(RESERVE_BYTES)
            .min(RESERVE_BYTES / 2 / 4 * 3)
            .min(DEFAULT_MAX_BUDGET);
        assert_eq!(none, 0);
    }

    #[test]
    fn runtime_guard_is_off_until_enabled() {
        // The guard must never fire on the judged path, so it is opt-in. The
        // call is a no-op here and must not panic even if memory is tight.
        enforce_runtime_floor();
        assert!(run_floor() > 0);
        assert_eq!(RUNTIME_CHECK_INTERVAL, 1 << 20);
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
