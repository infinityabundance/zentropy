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
    /// Raw two-field construction. Prefer the named form constructors below.
    pub fn new(compressor_bytes: u64, archive_bytes: u64) -> Self {
        Score {
            compressor_bytes,
            archive_bytes,
        }
    }

    /// The primary self-extracting form.
    ///
    /// Rule: `S := length(comp9.exe) + length(archive9.exe)`, where `comp9`
    /// reads enwik9 and writes `archive9`, and `archive9` reconstructs enwik9.
    pub fn self_extracting(comp9_bytes: u64, archive9_bytes: u64) -> Self {
        Score {
            compressor_bytes: comp9_bytes,
            archive_bytes: archive9_bytes,
        }
    }

    /// The relaxed separate-file form with distinct programs.
    ///
    /// Rule: `S := length(comp9a) + 2 x length(decomp9) + length(archive9.bhm)`.
    pub fn separate(comp9a_bytes: u64, decomp9_bytes: u64, bhm_bytes: u64) -> Self {
        Score {
            // The decompressor is charged twice; keep the two physical copies
            // explicit in `compressor_bytes` so the total is never misread.
            compressor_bytes: comp9a_bytes + 2 * decomp9_bytes,
            archive_bytes: bhm_bytes,
        }
    }

    /// The relaxed separate-file form when `comp9a == decomp9` (one program,
    /// used both ways). The rule reduces the `2 x` coefficient on `decomp9` to
    /// `1 x`, so the program is still charged **twice** in total:
    /// `S := length(P) + length(P) + length(archive9.bhm)`.
    pub fn shared_program(program_bytes: u64, bhm_bytes: u64) -> Self {
        Score {
            compressor_bytes: 2 * program_bytes,
            archive_bytes: bhm_bytes,
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

    #[test]
    fn submission_form_arithmetic() {
        let program = 313_792u64;
        let bhm = 276_263u64;
        let archive9 = 590_078u64; // program + marker(15) + length(8) + bhm

        // comp9a == decomp9: the rule reduces the 2x on decomp9 to 1x, so the
        // single program is charged twice in total: 2P + bhm.
        let shared = Score::shared_program(program, bhm);
        assert_eq!(shared.total(), 313_792 + 313_792 + 276_263);
        assert_eq!(shared.total(), 903_847);

        // Self-extracting: comp9 + archive9.
        let sfx = Score::self_extracting(program, archive9);
        assert_eq!(sfx.total(), 903_870);
        // The two legal packaging forms differ only by the SFX marker+length.
        assert_eq!(sfx.total() - shared.total(), 23);
    }

    #[test]
    fn separate_with_distinct_programs() {
        // Distinct comp9a and decomp9: comp9a + 2*decomp9 + bhm.
        let s = Score::separate(1_000, 2_000, 5_000);
        assert_eq!(s.total(), 1_000 + 4_000 + 5_000);
        assert_eq!(s.total(), 10_000);

        // The same-program case is a *rule reduction*, not a substitution into
        // the distinct-program formula: it is 2P + bhm, strictly less than
        // comp9a + 2*decomp9 + bhm when both equal P.
        let p = 7_000u64;
        let shared = Score::shared_program(p, 1_000);
        let naive = Score::separate(p, p, 1_000);
        assert_eq!(shared.total(), 2 * p + 1_000);
        assert!(shared.total() < naive.total());
    }
}
