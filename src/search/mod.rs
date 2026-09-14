//! Phase 9 — global search: the DSFB observer, frf-fuzz mutation and the
//! Gemel-memory interface.
//!
//! Three ideas, each borrowed from the wider Zentropy stack and re-derived here:
//!
//! * **Search has no decode authority** (`ZENTROPY_ARCHITECTURE.md` §3). Every
//!   knob this module explores is a *runtime* parameter carried in the archive
//!   header, so the decoder is unchanged and cannot be made incorrect by search.
//! * **frf-fuzz mutation**: a deterministic, seeded perturbation of the
//!   hyperparameter vector, so a campaign is reproducible from its seed alone.
//! * **DSFB observer**: deterministic structural signals (per-axis level means,
//!   the coordinate-wise optimum, and whether the axes cooperate) that say what
//!   to investigate next, as opposed to ranking configurations by noise.
//!
//! The Gemel memory is the append-only receipt log in [`crate::evidence`]; this
//! module provides the parsing and dedupe helpers so a campaign never pays twice
//! for the same configuration.

/// The APM adaptation shifts a `tune` byte selects, for receipts and diagnostics.
///
/// Owned by the coding layer ([`crate::context`]) because the scored decoder is
/// the authority on it. The axis is **REJECTED at enwik9** and compiled out by
/// default, in which case every tune reports the fixed pre-Phase-9 `(7, 7, 7)`.
pub fn apm_rates(tune: u8) -> (u32, u32, u32) {
    #[cfg(feature = "apm-tune")]
    {
        crate::context::APM_RATE_SETS[(tune >> 4) as usize]
    }
    #[cfg(not(feature = "apm-tune"))]
    {
        let _ = tune;
        (7, 7, 7)
    }
}

/// Size of the hyperparameter space carried in the `tune` byte.
pub const TUNE_SPACE: u16 = 256;

/// An axis of the runtime hyperparameter vector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    /// Mixer learning-rate ladder (A20).
    Lr,
    /// APM adaptation-shift set (Phase 9).
    Apm,
}

/// Every axis, in the canonical coordinate-descent order.
///
/// The APM axis is **REJECTED at enwik9** and only present under
/// `--features apm-tune`, so the default search is a one-dimensional descent over
/// the mixer learning-rate ladder.
#[cfg(feature = "apm-tune")]
pub const AXES: [Axis; 2] = [Axis::Lr, Axis::Apm];
#[cfg(not(feature = "apm-tune"))]
pub const AXES: [Axis; 1] = [Axis::Lr];

/// A point in the hyperparameter space: one level per axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Knobs {
    pub lr_idx: u8,
    pub apm_sel: u8,
}

impl Knobs {
    /// Decode a `tune` header byte.
    pub fn from_tune(t: u8) -> Self {
        Knobs {
            lr_idx: t & 15,
            apm_sel: t >> 4,
        }
    }

    /// Encode to the `tune` header byte.
    pub fn tune(self) -> u8 {
        ((self.apm_sel & 15) << 4) | (self.lr_idx & 15)
    }

    pub fn level(self, a: Axis) -> u8 {
        match a {
            Axis::Lr => self.lr_idx,
            Axis::Apm => self.apm_sel,
        }
    }

    pub fn set(self, a: Axis, v: u8) -> Self {
        match a {
            Axis::Lr => Knobs {
                lr_idx: v & 15,
                ..self
            },
            Axis::Apm => Knobs {
                apm_sel: v & 15,
                ..self
            },
        }
    }

    /// The coordinate neighbour one level along `axis` (wrapping). This is the
    /// deterministic step used by coordinate descent.
    pub fn neighbor(self, a: Axis, dir: i8) -> Self {
        let v = (self.level(a) as i8 + dir).rem_euclid(16) as u8;
        self.set(a, v)
    }
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// frf-fuzz mutation: a deterministic, seeded perturbation. Returns the mutated
/// point and the seed advanced, so a campaign is a pure function of its seed.
///
/// The axis is chosen modulo the number of *compiled-in* axes. That matters:
/// with the rejected APM axis compiled out there is only one axis, and indexing
/// the list with a raw bit would run off the end (it did — the search campaign
/// court caught it). `% 2` is identical to `& 1`, so the two-axis sequence that
/// produced the recorded APM screening is unchanged.
pub fn mutate(k: Knobs, seed: u64) -> (Knobs, u64) {
    let s = splitmix64(seed);
    let axis = AXES[((s >> 32) as usize) % AXES.len()];
    // Perturbation magnitude in levels, biased small: 1, 1, 2, 4 (wrapping).
    let mag: i8 = [1, 1, 2, 4][((s >> 24) & 3) as usize];
    let sign: i8 = if s & 1 == 0 { 1 } else { -1 };
    let next = k.neighbor(axis, sign * mag);
    (next, s)
}

/// One measured trial of the search.
#[derive(Clone, Debug, PartialEq)]
pub struct Trial {
    pub tune: u8,
    pub archive_bytes: u64,
    pub wall_ms: u64,
    pub exact: bool,
}

/// The best (minimum-archive) trial per tune value, ascending by archive bytes.
/// Ties are broken by the tune value so the result is deterministic.
pub fn best_per_tune(trials: &[Trial]) -> Vec<Trial> {
    let mut best: Vec<Option<Trial>> = vec![None; TUNE_SPACE as usize];
    for t in trials {
        let i = t.tune as usize;
        if i >= best.len() {
            continue;
        }
        match &best[i] {
            None => best[i] = Some(t.clone()),
            Some(b) if t.archive_bytes < b.archive_bytes => best[i] = Some(t.clone()),
            _ => {}
        }
    }
    let mut v: Vec<Trial> = best.into_iter().flatten().collect();
    v.sort_by(|a, b| {
        a.archive_bytes
            .cmp(&b.archive_bytes)
            .then(a.tune.cmp(&b.tune))
    });
    v
}

/// The Pareto frontier of a trial set. Every trial has identical binary cost (the
/// knob is one header byte already present), so the objective is simply the
/// minimum archive size; the frontier is reported with the full ranking up to and
/// including the best.
pub fn pareto(trials: &[Trial]) -> Vec<Trial> {
    let v = best_per_tune(trials);
    let Some(best) = v.first().map(|t| t.archive_bytes) else {
        return Vec::new();
    };
    v.into_iter()
        .take_while(|t| t.archive_bytes == best)
        .collect()
}

/// The DSFB observer: deterministic structural signals from a trial set.
#[derive(Clone, Debug)]
pub struct Observation {
    pub n: usize,
    pub best_tune: u8,
    pub best_archive: u64,
    /// Mean archive bytes at each level of each axis (axis-major).
    pub lr_mean: [f64; 16],
    pub apm_mean: [f64; 16],
    pub best_lr_idx: u8,
    pub best_apm_sel: u8,
    /// True when the observed coordinate-wise optimum coincides with the joint
    /// optimum, i.e. the axes act additively and coordinate descent converges.
    pub coordinate_consistent: bool,
    /// Ranges of the per-axis means, as a crude interaction/variance signal.
    pub lr_spread: f64,
    pub apm_spread: f64,
}

/// Observe a trial set. Deterministic: pure function of the multiset of trials.
pub fn observe(trials: &[Trial]) -> Observation {
    let mut lr_sum = [0f64; 16];
    let mut lr_n = [0u64; 16];
    let mut apm_sum = [0f64; 16];
    let mut apm_n = [0u64; 16];
    for t in trials {
        let k = Knobs::from_tune(t.tune);
        lr_sum[k.lr_idx as usize] += t.archive_bytes as f64;
        lr_n[k.lr_idx as usize] += 1;
        apm_sum[k.apm_sel as usize] += t.archive_bytes as f64;
        apm_n[k.apm_sel as usize] += 1;
    }
    let mut lr_mean = [0f64; 16];
    let mut apm_mean = [0f64; 16];
    for i in 0..16 {
        lr_mean[i] = if lr_n[i] > 0 {
            lr_sum[i] / lr_n[i] as f64
        } else {
            f64::INFINITY
        };
        apm_mean[i] = if apm_n[i] > 0 {
            apm_sum[i] / apm_n[i] as f64
        } else {
            f64::INFINITY
        };
    }
    let argmin = |m: &[f64; 16]| -> u8 {
        let mut bi = 0usize;
        for i in 1..16 {
            if m[i] < m[bi] {
                bi = i;
            }
        }
        bi as u8
    };
    let best_lr_idx = argmin(&lr_mean);
    let best_apm_sel = argmin(&apm_mean);
    let (best_tune, best_archive) = trials
        .iter()
        .min_by(|a, b| {
            a.archive_bytes
                .cmp(&b.archive_bytes)
                .then(a.tune.cmp(&b.tune))
        })
        .map(|t| (t.tune, t.archive_bytes))
        .unwrap_or((0, 0));
    let spread = |m: &[f64; 16]| -> f64 {
        let finite: Vec<f64> = m.iter().copied().filter(|x| x.is_finite()).collect();
        if finite.len() < 2 {
            return 0.0;
        }
        let lo = finite.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = finite.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        hi - lo
    };
    let coord = Knobs {
        lr_idx: best_lr_idx,
        apm_sel: best_apm_sel,
    };
    Observation {
        n: trials.len(),
        best_tune,
        best_archive,
        lr_mean,
        apm_mean,
        best_lr_idx,
        best_apm_sel,
        coordinate_consistent: coord.tune() == best_tune,
        lr_spread: spread(&lr_mean),
        apm_spread: spread(&apm_mean),
    }
}

/// Extract a string field from a minimal JSON object line, e.g. `"tune":"7"`.
/// Deliberately tiny: receipts are written by us and are flat string maps.
pub fn json_field(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = line.find(&pat)? + pat.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Parse search receipts into trials for a given method. Lines that are not
/// search trials for the method are ignored (so the same log may hold every
/// experiment in the project).
///
/// When `corpus_sha` is `Some`, only trials whose recorded `input_sha256`
/// matches are kept: a tune measured on one corpus is *not* evidence about
/// another, and conflating them would make the memory silently drop a
/// configuration that was never actually tried here.
pub fn trials_from_lines_for(
    lines: &[String],
    method: &str,
    corpus_sha: Option<&str>,
) -> Vec<Trial> {
    let mut out = Vec::new();
    for line in lines {
        if json_field(line, "attribution").as_deref() != Some("search/tune") {
            continue;
        }
        if json_field(line, "method").as_deref() != Some(method) {
            continue;
        }
        if let Some(want) = corpus_sha {
            if json_field(line, "input_sha256").as_deref() != Some(want) {
                continue;
            }
        }
        let (Some(tune), Some(bytes)) = (
            json_field(line, "tune").and_then(|s| s.parse::<u8>().ok()),
            json_field(line, "archive_bytes").and_then(|s| s.parse::<u64>().ok()),
        ) else {
            continue;
        };
        let wall_ms = json_field(line, "wall_seconds")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| (s * 1000.0) as u64)
            .unwrap_or(0);
        let exact = json_field(line, "exact").as_deref() == Some("true");
        out.push(Trial {
            tune,
            archive_bytes: bytes,
            wall_ms,
            exact,
        });
    }
    out
}

/// [`trials_from_lines_for`] over every corpus in the log.
pub fn trials_from_lines(lines: &[String], method: &str) -> Vec<Trial> {
    trials_from_lines_for(lines, method, None)
}

/// The tunes already measured for `method` *on the given corpus* in a receipt
/// log — the Gemel memory query that stops a campaign re-paying for a known
/// configuration.
pub fn already_tried_for(lines: &[String], method: &str, corpus_sha: Option<&str>) -> Vec<u8> {
    let mut v: Vec<u8> = trials_from_lines_for(lines, method, corpus_sha)
        .into_iter()
        .map(|t| t.tune)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// [`already_tried_for`] over every corpus in the log (used when a caller has no
/// corpus digest to hand).
pub fn already_tried(lines: &[String], method: &str) -> Vec<u8> {
    already_tried_for(lines, method, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tune_roundtrips_over_the_whole_space() {
        for t in 0u16..TUNE_SPACE {
            assert_eq!(Knobs::from_tune(t as u8).tune(), t as u8);
        }
    }

    #[test]
    fn neighbor_steps_and_wraps() {
        let k = Knobs {
            lr_idx: 0,
            apm_sel: 15,
        };
        assert_eq!(k.neighbor(Axis::Lr, -1).lr_idx, 15);
        assert_eq!(k.neighbor(Axis::Apm, 1).apm_sel, 0);
        assert_eq!(k.neighbor(Axis::Lr, 2).level(Axis::Lr), 2);
    }

    #[test]
    fn mutation_is_deterministic_and_moves_one_axis() {
        let k = Knobs::from_tune(7);
        let (m1, s1) = mutate(k, 12345);
        let (m2, s2) = mutate(k, 12345);
        assert_eq!(m1, m2);
        assert_eq!(s1, s2);
        // Exactly one axis changes.
        let changed_lr = m1.lr_idx != k.lr_idx;
        let changed_apm = m1.apm_sel != k.apm_sel;
        assert!(
            changed_lr ^ changed_apm,
            "mutation must move exactly one axis"
        );
    }

    #[test]
    fn mutation_never_indexes_past_the_axis_list() {
        // Regression: with the rejected APM axis compiled out `AXES` has one
        // entry, and indexing it with a raw bit ran off the end and aborted the
        // guided schedule. Every seed must be in range in both configurations.
        let mut k = Knobs::from_tune(5);
        for seed in 0..4096u64 {
            let (next, _s) = mutate(k, seed);
            assert!(next.lr_idx < 16 && next.apm_sel < 16);
            k = next;
        }
    }

    #[test]
    fn pareto_picks_the_best_archive() {
        let trials = vec![
            Trial {
                tune: 1,
                archive_bytes: 100,
                wall_ms: 0,
                exact: true,
            },
            Trial {
                tune: 2,
                archive_bytes: 90,
                wall_ms: 0,
                exact: true,
            },
            Trial {
                tune: 3,
                archive_bytes: 90,
                wall_ms: 0,
                exact: true,
            },
            Trial {
                tune: 4,
                archive_bytes: 120,
                wall_ms: 0,
                exact: true,
            },
        ];
        let p = pareto(&trials);
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|t| t.archive_bytes == 90));
    }

    #[test]
    fn observer_finds_the_coordinate_optimum() {
        // Construct a separable response: lr level 3 best, apm level 9 best.
        let mut trials = Vec::new();
        for lr in 0..16u8 {
            for apm in 0..16u8 {
                let cost = 1000
                    + (lr as i64 - 3).unsigned_abs() as u64 * 10
                    + (apm as i64 - 9).unsigned_abs() as u64 * 7;
                trials.push(Trial {
                    tune: Knobs {
                        lr_idx: lr,
                        apm_sel: apm,
                    }
                    .tune(),
                    archive_bytes: cost,
                    wall_ms: 0,
                    exact: true,
                });
            }
        }
        let o = observe(&trials);
        assert_eq!(o.best_lr_idx, 3);
        assert_eq!(o.best_apm_sel, 9);
        assert!(o.coordinate_consistent);
        assert_eq!(
            o.best_tune,
            Knobs {
                lr_idx: 3,
                apm_sel: 9
            }
            .tune()
        );
    }

    #[test]
    fn receipt_parsing_extracts_trials() {
        let line = r#"{"id":"search/x","tune":"7","archive_bytes":"1234","wall_seconds":"1.500000","exact":"true","attribution":"search/tune","method":"residual"}"#.to_string();
        let other = r#"{"id":"eval/y","attribution":"method=phase4 parent=column"}"#.to_string();
        let lines = vec![line, other];
        let ts = trials_from_lines(&lines, "residual");
        assert_eq!(ts.len(), 1);
        assert_eq!(ts[0].tune, 7);
        assert_eq!(ts[0].archive_bytes, 1234);
        assert_eq!(ts[0].wall_ms, 1500);
        assert!(ts[0].exact);
        assert_eq!(
            json_field(&lines[0], "archive_bytes").as_deref(),
            Some("1234")
        );
    }

    #[test]
    fn apm_axis_is_inert_without_the_feature() {
        // The APM axis is REJECTED at enwik9 and compiled out by default. With it
        // out, two tunes that differ only in the high nibble are the same
        // configuration and the search has one axis; with `--features apm-tune`
        // the second axis comes back for reproduction.
        #[cfg(not(feature = "apm-tune"))]
        {
            assert_eq!(apm_rates(6), apm_rates(22));
            assert_eq!(apm_rates(6), (7, 7, 7));
            assert_eq!(AXES.len(), 1);
        }
        #[cfg(feature = "apm-tune")]
        {
            assert_eq!(apm_rates(134), (6, 7, 6));
            assert_eq!(AXES.len(), 2);
        }
    }

    #[test]
    fn tried_tunes_are_reported() {
        let lines = vec![
            r#"{"tune":"5","archive_bytes":"10","attribution":"search/tune","method":"residual"}"#
                .to_string(),
            r#"{"tune":"5","archive_bytes":"10","attribution":"search/tune","method":"residual"}"#
                .to_string(),
            r#"{"tune":"9","archive_bytes":"10","attribution":"search/tune","method":"residual"}"#
                .to_string(),
        ];
        assert_eq!(already_tried(&lines, "residual"), vec![5, 9]);
        assert!(already_tried(&lines, "other").is_empty());
    }

    #[test]
    fn memory_is_scoped_to_the_corpus_digest() {
        let lines = vec![
            r#"{"tune":"5","archive_bytes":"10","input_sha256":"aa","attribution":"search/tune","method":"residual"}"#.to_string(),
            r#"{"tune":"9","archive_bytes":"10","input_sha256":"bb","attribution":"search/tune","method":"residual"}"#.to_string(),
        ];
        // A tune measured on another corpus is not evidence about this one.
        assert_eq!(already_tried_for(&lines, "residual", Some("aa")), vec![5]);
        assert_eq!(already_tried_for(&lines, "residual", Some("bb")), vec![9]);
        assert!(already_tried_for(&lines, "residual", Some("cc")).is_empty());
        // With no digest, every corpus in the log counts.
        assert_eq!(already_tried_for(&lines, "residual", None), vec![5, 9]);
    }
}
