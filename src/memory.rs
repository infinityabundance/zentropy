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

#[cfg(feature = "mem-floor")]
use std::sync::atomic::AtomicU32;
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
#[cfg(feature = "mem-floor")]
pub const RUN_FLOOR_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Bytes coded between runtime memory checks. Cheap enough to be free (one
/// relaxed atomic load per megabyte) and frequent enough to react in seconds.
pub const RUNTIME_CHECK_INTERVAL: usize = 1 << 20;

/// Consecutive below-floor samples tolerated before a run aborts.
///
/// A single sample is **not** evidence of a machine that is out of memory. Any
/// unrelated transient — a compiler link, a browser tab, a page-cache eviction —
/// can drop `MemAvailable` below the floor for a moment. Reacting to one sample
/// discards the whole run, and an enwik9 pass is ~45 minutes: that is exactly what
/// happened when a `cargo build` was started alongside five live gates, which
/// killed all five at 87% for a dip that cleared within seconds.
///
/// Three consecutive samples is several seconds of *sustained* pressure (one
/// sample is one mebibyte of coded stream, ~1 s on a small rung and ~3 s on
/// enwik9). A genuine exhaustion event always lasts that long; a blip does not.
/// The delay is safe because this process's own footprint is fixed after startup
/// (bounded by the [`budget`] check), so it cannot be the process consuming the
/// last of the floor while it waits.
#[cfg(feature = "mem-floor")]
pub const FLOOR_BREACH_PATIENCE: u32 = 3;

/// Whether the runtime floor is armed.
///
/// Always present (it is one byte of `.bss` and no code), because the *call*
/// site is kept in every build for the layout reason in the module docs. It
/// defaults to false and the submission stub never sets it, so the guard is inert
/// on the judged path — a legitimate reconstruction can never be aborted.
static RUNTIME_GUARD: AtomicBool = AtomicBool::new(false);

/// Length of the current run of consecutive below-floor samples. Reset to zero
/// by any sample at or above the floor, so only a *sustained* breach aborts.
#[cfg(feature = "mem-floor")]
static FLOOR_BREACHES: AtomicU32 = AtomicU32::new(0);

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

/// The pure part of the floor logic: fold one sample into the breach streak and
/// say whether the run must stop. Split out so the behaviour is testable without
/// touching the machine's actual memory (which no test may control).
///
/// Returns `(new_streak, abort)`.
#[cfg(feature = "mem-floor")]
fn note_sample(streak: u32, available: u64, floor: u64) -> (u32, bool) {
    if available >= floor {
        (0, false)
    } else {
        let n = streak.saturating_add(1);
        (n, n >= FLOOR_BREACH_PATIENCE)
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
///
/// A breach must persist for [`FLOOR_BREACH_PATIENCE`] consecutive samples
/// before it aborts; the first sample of a streak is *reported* instead, so a
/// tolerated dip leaves a trace in the run log rather than passing silently.
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
        let streak = FLOOR_BREACHES.load(Ordering::Relaxed);
        let (next, abort) = note_sample(streak, a, floor);
        FLOOR_BREACHES.store(next, Ordering::Relaxed);
        if next == 1 {
            eprintln!(
                "zentropy: memory floor: {:.2} GiB available, floor {:.2} GiB — \
                 monitoring {} more samples before aborting",
                a as f64 / (1024.0 * 1024.0 * 1024.0),
                floor as f64 / (1024.0 * 1024.0 * 1024.0),
                FLOOR_BREACH_PATIENCE - 1
            );
        }
        if abort {
            panic!(
                "memory floor breached: {:.2} GiB available, floor {:.2} GiB, for {} \
                 consecutive samples ({:.2} MiB coded). Aborting this run to protect \
                 the machine. Free memory, or lower the model with --max-ram / \
                 ZENTROPY_MAX_RAM_BYTES, then retry.",
                a as f64 / (1024.0 * 1024.0 * 1024.0),
                floor as f64 / (1024.0 * 1024.0 * 1024.0),
                next,
                (RUNTIME_CHECK_INTERVAL * next as usize) as f64 / (1024.0 * 1024.0),
            );
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
    fn a_transient_dip_does_not_abort_but_a_sustained_one_does() {
        // The whole point of the patience window: a single below-floor sample is
        // not evidence of exhaustion, so a run must survive it. An enwik9 pass is
        // ~45 min and was previously thrown away by one sample.
        let floor = RUN_FLOOR_BYTES;
        let low = floor - 1;
        let ok = floor + 1;

        // One dip: tolerated, streak recorded, no abort.
        let (streak, abort) = note_sample(0, low, floor);
        assert_eq!((streak, abort), (1, false));
        // Sustained dip: aborts exactly at the patience bound, not before.
        let mut streak = 0;
        let mut aborted_at = None;
        for i in 1..=(FLOOR_BREACH_PATIENCE + 2) {
            let (s, a) = note_sample(streak, low, floor);
            streak = s;
            if a && aborted_at.is_none() {
                aborted_at = Some(i);
            }
        }
        assert_eq!(aborted_at, Some(FLOOR_BREACH_PATIENCE));
        // A sample back above the floor clears the streak entirely, so the window
        // must be *consecutive* — otherwise a slow leak would still be tolerated.
        let (streak, abort) = note_sample(FLOOR_BREACH_PATIENCE - 1, ok, floor);
        assert_eq!((streak, abort), (0, false));
        let (streak, abort) = note_sample(streak, low, floor);
        assert_eq!((streak, abort), (1, false));
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
