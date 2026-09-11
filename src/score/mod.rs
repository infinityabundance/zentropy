//! The objective function. `S` is authority; everything else is a diagnostic.
//!
//! ```text
//! S = submitted_compressor_bytes + self_extracting_archive_bytes
//! ```
//!
//! subject to `decode(archive9) == enwik9` byte-for-byte and every current
//! Hutter resource/portability/self-containment constraint. A mechanism is
//! beneficial only if its complete marginal cost `ΔS(M) < 0`. This module makes
//! that arithmetic impossible to get subtly wrong: it is the single place where
//! a score is computed, and every experiment receipt binds one.

use std::fmt;

/// Hutter Prize resource envelope, evaluated against a specific machine.
///
/// The rules scale the wall-clock allowance by the machine's Geekbench 5 score:
/// a program must run in less than `70_000 / T` hours, where `T` is the
/// single-core Geekbench 5 score of the test machine. With `T = 1427` that is
/// ~49.05 hours, matching the published "≲50 hours on a single core".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResourceLimits {
    /// Peak resident memory ceiling, bytes. Rule: at most 10 GB.
    pub max_ram_bytes: u64,
    /// Temporary disk ceiling, bytes. Rule: at most 100 GB.
    pub max_temp_disk_bytes: u64,
    /// Single-core Geekbench 5 score of the reference machine.
    pub geekbench5: f64,
    /// CPU cores assumed by the judged path. The rules judge single-core.
    pub cores: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        ResourceLimits {
            max_ram_bytes: 10_000_000_000,
            max_temp_disk_bytes: 100_000_000_000,
            // The slower of the two currently published test machines; using
            // the conservative value keeps us honest about runtime.
            geekbench5: 1310.0,
            cores: 1,
        }
    }
}

impl ResourceLimits {
    /// Seconds allowed for each of compression and decompression.
    pub fn time_limit_seconds(&self) -> f64 {
        70_000.0 / self.geekbench5 * 3600.0
    }

    pub fn time_limit_hours(&self) -> f64 {
        70_000.0 / self.geekbench5
    }
}

/// A measured or projected Hutter score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Score {
    /// Bytes of the submission's compressor (or `comp9.exe`).
    pub compressor_bytes: u64,
    /// Bytes of the self-extracting archive (`archive9.exe`/`archive9.bhm`).
    pub archive_bytes: u64,
}

impl Score {
    pub fn new(compressor_bytes: u64, archive_bytes: u64) -> Self {
        Score {
            compressor_bytes,
            archive_bytes,
        }
    }

    /// `S`, the quantity the prize minimises.
    pub fn total(&self) -> u64 {
        self.compressor_bytes + self.archive_bytes
    }

    /// Compression ratio against the canonical 10^9-byte corpus.
    pub fn ratio(&self) -> f64 {
        crate::corpus::ENWIK9_LEN as f64 / self.total() as f64
    }

    /// Bits per input byte against the canonical corpus.
    pub fn bits_per_byte(&self) -> f64 {
        (self.total() as f64 * 8.0) / crate::corpus::ENWIK9_LEN as f64
    }
}

impl fmt::Display for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "S={} (comp={} + archive={})",
            self.total(),
            self.compressor_bytes,
            self.archive_bytes
        )
    }
}

/// The record against which a new submission is judged, and the derived gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    /// `L`: the preceding record's total score.
    pub previous_total: u64,
    /// Human label, e.g. `"fx2-cmix (accepted 2024-10-08)"`.
    pub label: &'static str,
}

impl Record {
    /// The prize gate: a submission must beat `L` by at least 1%.
    ///
    /// Rule: "Award = Z×(L-S)/L ... Minimum claim is 5'000€ (1% improvement)."
    /// So the maximum `S` that still qualifies is `floor(0.99 × L)`.
    pub fn gate_total(&self) -> u64 {
        ((self.previous_total as u128 * 99) / 100) as u64
    }

    /// Whether `S` clears the 1% hurdle.
    pub fn qualifies(&self, s: Score) -> bool {
        s.total() < self.gate_total() && s.total() < self.previous_total
    }

    /// Award in euros for `S`, using `z` as the (constant) prize fund.
    pub fn award_euros(&self, s: Score, z: f64) -> f64 {
        if s.total() >= self.previous_total {
            return 0.0;
        }
        let raw = z * (self.previous_total as f64 - s.total() as f64) / self.previous_total as f64;
        raw.max(5_000.0)
    }
}

/// The three targets the project tracks, per the Phase 0 constitution.
///
/// `T0` is the currently accepted official record. `T1` is the strongest
/// credible pending/verified frontier — for legal purposes the *gate* is always
/// computed from `T0`, but knowing `T1` prevents overfitting to a stale record.
/// `T2` is an engineering moonshot, not a claim that it is attainable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Targets {
    pub t0: Record,
    pub t1: u64,
    pub t2: u64,
}

impl Targets {
    /// The snapshot pinned in `docs/COMPETITIVE_BASELINE.md`.
    ///
    /// - T0: fx2-cmix total score, accepted 2024-10-08: 110,351,665 (archive9)
    ///   + 441,468 (compressor) = 110,793,128.
    /// - T1: fx2-cmix-transformer archive9 = 96,996,198 plus its 3,426,642-byte
    ///   compressor => 100,422,840, consistent with the reported
    ///   Hutter-score of 100,424,672 (the small difference is the compressor
    ///   build used in the scored run). We pin the conservative value.
    /// - T2: 95,000,000 bytes, the internal moonshot.
    pub fn pinned() -> Self {
        Targets {
            t0: Record {
                previous_total: 110_793_128,
                label: "fx2-cmix (accepted 2024-10-08)",
            },
            t1: 100_424_672,
            t2: 95_000_000,
        }
    }

    /// The active prize gate derived from the accepted record.
    pub fn gate(&self) -> u64 {
        self.t0.gate_total()
    }
}

/// Why a candidate is or is not eligible. Optimisation happens only over
/// eligible candidates; ineligible ones may be kept as `RESEARCH_ONLY`.
#[derive(Debug, Clone, PartialEq)]
pub enum Eligibility {
    Eligible,
    Ineligible(Vec<Ineligibility>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Ineligibility {
    ReconstructionInexact,
    RuntimeExceeded { seconds: f64, limit: f64 },
    RamExceeded { bytes: u64, limit: u64 },
    DiskExceeded { bytes: u64, limit: u64 },
    GpuUsedInJudgedPath,
    NotSelfContained,
    RuleViolation(String),
}

impl Eligibility {
    pub fn is_eligible(&self) -> bool {
        matches!(self, Eligibility::Eligible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_is_one_percent_below_record() {
        let t = Targets::pinned();
        assert_eq!(t.gate(), 109_685_196); // floor(0.99 * 110,793,128)
        assert!(t.gate() < t.t0.previous_total);
    }

    #[test]
    fn qualifying_boundary() {
        let t = Targets::pinned();
        // The gate is on the *total* S, not on any single component.
        let pass = Score::new(0, t.gate() - 1);
        let fail = Score::new(0, t.gate());
        assert!(t.t0.qualifies(pass));
        assert!(!t.t0.qualifies(fail));
        // A compressor/archive split that sums below the gate also passes.
        assert!(t.t0.qualifies(Score::new(1_000_000, t.gate() - 1_000_001)));
    }

    #[test]
    fn award_formula() {
        let t = Targets::pinned();
        // A submission exactly at the 1% gate: (L - 0.99L)/L = 0.01 => 5000 for Z=500k.
        let s = Score::new(0, t.gate());
        let award = t.t0.award_euros(s, 500_000.0);
        assert!((award - 5000.0).abs() < 1.0, "award={award}");
    }

    #[test]
    fn geekbench_scaling() {
        let l = ResourceLimits {
            geekbench5: 1427.0,
            ..Default::default()
        };
        // 70000/1427 = 49.05 hours.
        assert!((l.time_limit_hours() - 49.05).abs() < 0.1);
    }
}
