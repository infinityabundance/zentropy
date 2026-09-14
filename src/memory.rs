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
//!    death.
//!
//! Layers 1–2 are the **scored** protection (feature `mem-guard`): cheap, and a
//! refusal is always safe. Layer 3 is **research only** (feature `mem-floor`):
//! its *implementation* — `/proc` parsing, the patience state and the abort text
//! — is compiled out of the scored stub, because the judged decoder never arms it
//! and a mid-reconstruction abort would fail a run for no benefit.
//!
//! What stays in every build is the *call site*: the coding loops call
//! [`enforce_runtime_floor`] unconditionally and it tests one relaxed atomic. That
//! keeps `archive.rs` free of `cfg` sprawl and costs nothing measurable. It is
//! worth recording why, because the measurement was counter-intuitive:
//! `opt-level="z"` is not monotone in code size, and removing the floor's body
//! made the scored stub **256 B larger** (399,632 → 399,888, three identical
//! repetitions). Gating only the body did *not* recover the smaller size, so the
//! lever is the body's effect on LLVM's inliner rather than the call itself. The
//! 256 B is a layout artifact, not a mechanism price; it is charged honestly and
//! left to Phase 11, which restructures the stub and must re-measure. See
//! `docs/MEMORY_GUARD.md` §4.
//!
//! A hard kernel-enforced ceiling (`ulimit -v`) is applied by the long-run
//! wrappers in `tools/`; see `tools/p9_gate.sh`.

use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "mem-floor")]
use std::time::{Duration, Instant};

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

/// Available memory below which a *running* research job pauses.
///
/// The distinction from [`RESERVE_BYTES`] matters: the reserve stops a run from
/// *starting* when the machine is already tight, while this floor stops a run
/// that was fine at startup from thrashing the machine an hour later.
#[cfg(feature = "mem-floor")]
pub const RUN_FLOOR_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Bytes coded between runtime memory checks. Cheap enough to be free (one
/// relaxed atomic load per megabyte) and frequent enough to react in seconds.
pub const RUNTIME_CHECK_INTERVAL: usize = 1 << 20;

/// How long a run may sit below the floor before giving up and aborting.
///
/// The floor *pauses* rather than aborting (see [`floor_action`]), because a run
/// late in a 90-minute pass must not be discarded because an unrelated process
/// spiked. Ten minutes is long enough to ride out anything that clears, and short
/// enough that a genuinely exhausted machine still gets its memory back.
#[cfg(feature = "mem-floor")]
pub const FLOOR_PATIENCE: Duration = Duration::from_secs(600);

/// How long to sleep between re-checks while paused.
#[cfg(feature = "mem-floor")]
pub const FLOOR_POLL: Duration = Duration::from_secs(5);

/// What the runtime floor does about one memory sample.
#[cfg(feature = "mem-floor")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FloorAction {
    /// Enough memory: keep coding.
    Continue,
    /// Below the floor, but not for long enough to give up: sleep and re-check.
    Sleep,
    /// Below the floor for [`FLOOR_PATIENCE`]: abort and release the memory.
    Abort,
}

/// The floor's decision, as a pure function so it is testable without any
/// control over the machine's actual memory.
#[cfg(feature = "mem-floor")]
#[inline]
pub fn floor_action(available: u64, floor: u64, paused_for: Duration) -> FloorAction {
    if available >= floor {
        FloorAction::Continue
    } else if paused_for >= FLOOR_PATIENCE {
        FloorAction::Abort
    } else {
        FloorAction::Sleep
    }
}

/// Whether the runtime floor is armed.
///
/// Always present (it is one byte of `.bss` and no code), because the *call*
/// site is kept in every build for the layout reason in the module docs. It
/// defaults to false and the submission stub never sets it, so the guard is inert
/// on the judged path — a legitimate reconstruction can never be paused or
/// aborted.
static RUNTIME_GUARD: AtomicBool = AtomicBool::new(false);

/// Arm the runtime memory floor. The research driver calls this once at startup;
/// the submission stub never does. An unarmed guard is a no-op in every build.
pub fn enable_runtime_guard() {
    RUNTIME_GUARD.store(true, Ordering::Relaxed);
}

/// The running floor, honouring `ZENTROPY_RUN_FLOOR_BYTES`.
#[cfg(feature = "mem-floor")]
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
#[cfg(feature = "mem-floor")]
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
/// path and nothing on the judged path.
///
/// When the floor is breached the run **pauses** rather than aborting: it sleeps
/// [`FLOOR_POLL`] and re-checks, and gives up only after [`FLOOR_PATIENCE`] of
/// sustained pressure. Two incidents on 2026-09-14 discarded five concurrent
/// enwik9 gates at 87% each time, because the old rule aborted on a single
/// sample; the second incident proved the dips were sustained, so a longer
/// patience on its own would *not* have saved them. Pausing does, and it is
/// legitimate precisely because coding is deterministic and time-independent:
/// the same bytes are coded the same way before and after the sleep.
///
/// The function itself is present in every build, so the call sites need no
/// `cfg`; only the implementation is gated, and a stub that never arms the guard
/// therefore never carries the `/proc` parsing or the abort text. See the module
/// docs for the measured, and counter-intuitive, size consequence of that choice.
#[inline]
pub fn enforce_runtime_floor() {
    if !RUNTIME_GUARD.load(Ordering::Relaxed) {
        return;
    }
    #[cfg(feature = "mem-floor")]
    {
        let floor = run_floor();
        let Some(a) = available_bytes() else {
            return;
        };
        // Fast path: the overwhelmingly common case, one load and one compare.
        if floor_action(a, floor, Duration::ZERO) == FloorAction::Continue {
            return;
        }
        // Below the floor. **Pause rather than discard.** Coding is a pure
        // function of the input and the model state — no clock, no randomness, no
        // shared mutable state — so suspending here cannot change the output, and
        // the decoder that re-runs the same bytes makes the same decision. That is
        // what makes a pause legitimate where an abort was merely safe.
        let start = Instant::now();
        eprintln!(
            "zentropy: memory floor: {:.2} GiB available, floor {:.2} GiB — pausing \
             (up to {}s) rather than discarding this run",
            a as f64 / (1024.0 * 1024.0 * 1024.0),
            floor as f64 / (1024.0 * 1024.0 * 1024.0),
            FLOOR_PATIENCE.as_secs()
        );
        loop {
            std::thread::sleep(FLOOR_POLL);
            let now = available_bytes().unwrap_or(u64::MAX);
            match floor_action(now, floor, start.elapsed()) {
                FloorAction::Continue => {
                    eprintln!(
                        "zentropy: memory floor: {:.2} GiB available — resuming after {:.1}s",
                        now as f64 / (1024.0 * 1024.0 * 1024.0),
                        start.elapsed().as_secs_f64()
                    );
                    return;
                }
                FloorAction::Sleep => {}
                FloorAction::Abort => panic!(
                    "memory floor breached for {}s: {:.2} GiB available, floor {:.2} GiB. \
                     Aborting to release this run's memory. Free memory, or lower the \
                     model with --max-ram / ZENTROPY_MAX_RAM_BYTES, then retry.",
                    start.elapsed().as_secs(),
                    now as f64 / (1024.0 * 1024.0 * 1024.0),
                    floor as f64 / (1024.0 * 1024.0 * 1024.0),
                ),
            }
        }
    }
}

/// A one-line human summary of the current memory situation.
#[cfg(feature = "mem-floor")]
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

    #[cfg(feature = "mem-floor")]
    #[test]
    fn runtime_guard_is_off_until_enabled() {
        // The guard must never fire on the judged path, so it is opt-in. The
        // call is a no-op here and must not panic even if memory is tight.
        enforce_runtime_floor();
        assert!(run_floor() > 0);
        assert_eq!(RUNTIME_CHECK_INTERVAL, 1 << 20);
    }

    #[cfg(feature = "mem-floor")]
    #[test]
    fn a_transient_dip_pauses_and_recovers_instead_of_aborting() {
        // The whole point of pausing: a machine that tightens for a moment must
        // not cost a 90-minute pass. Five concurrent enwik9 gates were discarded
        // twice on 2026-09-14 by an abort rule the machine did not deserve.
        let floor = RUN_FLOOR_BYTES;
        let low = floor - 1;
        let ok = floor + 1;

        // A dip pauses immediately...
        assert_eq!(floor_action(low, floor, Duration::ZERO), FloorAction::Sleep);
        // ...and resumes the moment memory recovers, however long it took.
        assert_eq!(
            floor_action(ok, floor, FLOOR_PATIENCE * 2),
            FloorAction::Continue
        );
        // Exactly at the floor is fine: the comparison is inclusive.
        assert_eq!(
            floor_action(floor, floor, Duration::ZERO),
            FloorAction::Continue
        );
        // Sustained pressure is only given up on after the patience window, so the
        // abort is a last resort rather than the first response.
        assert_eq!(
            floor_action(low, floor, FLOOR_PATIENCE - Duration::from_secs(1)),
            FloorAction::Sleep
        );
        assert_eq!(floor_action(low, floor, FLOOR_PATIENCE), FloorAction::Abort);
        // The window is bounded, so a genuinely exhausted machine recovers.
        assert!(FLOOR_PATIENCE >= FLOOR_POLL * 2);
        assert!(FLOOR_POLL > Duration::ZERO);
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
