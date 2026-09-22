//! Hierarchical context mixing: families with local mixers, one context-selected
//! top mixer, and the instrumentation §14.33 asks for.
//!
//! `docs/PHASE14_FIRST_BOUNDARY.md` §6 answers where the codelength actually is:
//! lexical prose is 76% of it, and the model is already 2× below order-0 on it.
//! The procedural representation family was measured and rejected, so the
//! remaining pool is *better modelling of expensive running text*. §14.32/§14.33
//! are the redirect: stop putting every expert into one flat global competition,
//! and instead let **families** compete — a local mixer per family learns which of
//! *its* experts to trust, and a top mixer learns which family to trust *given
//! what kind of data this is*.
//!
//! The reason the flat [`Mixer`] cannot express this: it only sees the inputs, so
//! a family that is internally consistent but individually weak has to out-shout
//! a family of individually loud experts. **Expert disagreement is information.**
//! A local mixer compresses a family into one prediction plus a residual
//! confidence; the top-level context (`c0`, structural class, partial-byte state,
//! word state, a bucket derived from disagreement) then decides which compressed
//! family to believe. The flat mixer has to relearn every family-vs-family
//! interaction in every structural context; the hierarchy shares it.
//!
//! # What is here
//!
//! * [`HierMixer`] — `n` families, each a [`Family`] `(name, start, len)` over a
//!   shared input roster, each with its own single-weight-set local [`Mixer`]; a
//!   top [`Mixer`] over the stretched family outputs whose weights are selected
//!   by a caller-supplied `u32` context. The same code serves every roster, so a
//!   caller declares families once and the topology follows.
//! * [`FamilyStats`] — per-family activation count, per-family log loss,
//!   marginal contribution (the change in top-level loss when the family is
//!   removed) and weight distribution. A family that adds nothing shows up as
//!   `marginal_bits ≈ 0`, not as a plausible-looking number.
//! * [`compare_flat_vs_hier`] — the measured comparison against the flat
//!   [`Mixer`] on a slice, at equal input count and comparable weight memory.
//!
//! # The mixing context
//!
//! [`HierMixer::mix`] takes a `u32`. It is folded exactly the way the existing
//! [`Mixer`] folds its own context — `ctx as usize % n_ctx` — so a caller can
//! pass structural class, partial-byte state, word state, or a bucket derived
//! from expert disagreement and it selects the top-level weight set directly.
//! The caller must supply a value that is a **pure function of data already
//! coded**: the decoder computes the same context, so encode and decode cannot
//! disagree. `signal::SignalState` is the intended production source, but it is
//! gated behind `procedural`/`opportunity`, so this module deliberately accepts a
//! plain `u32` and never depends on it — see [`HierMixer::mix`].
//!
//! # Honest result
//!
//! **The hierarchy does not win at this scale.** On a 1 MiB slice of the
//! development rung (`evidence/corpus/enwik6`) with the 16 experts above in five
//! families, equal input count and weight memory matched to within 0.07%, the
//! flat [`Mixer`] codes the slice strictly cheaper than the two-level mixer:
//!
//! ```text
//! flat  ctx=1024  weights=65,536 B  ideal=2,290,008.0 bits (2.2900 bpc)
//! hier  top_ctx=3276 weights=65,584 B ideal=2,366,244.9 bits (2.3662 bpc)
//! delta = +76,236.9 bits (+0.0762 bpc): the hierarchy LOSES
//! ```
//!
//! Measured with
//! `cargo test --release --features accepted --lib hier::tests::enwik6_report -- --ignored --nocapture`.
//! The per-family instrumentation shows why the loss is not a bug in the
//! grouping: every family is active and every family has a *positive* marginal
//! contribution (`order/byte +0.0754`, `word/lexical +0.0250`, `structural
//! +0.0242`, `match +0.0275`, `ppm +0.0029` bits/bit), so the families are real
//! and separable. The hierarchy loses anyway because its local mixers duplicate
//! adaptation the flat mixer already performs for free, while the flat mixer gets
//! all 16 experts to select among in every context — at 1 MiB the extra
//! two-level parameter surface is simply not paid back. The `ppm` row is the
//! warning §14.33 asks the instrumentation to surface: a family that adds almost
//! nothing (`+0.0029` b/bit) is visible as adding almost nothing.
//!
//! This is a diagnostic, not a claim of adoption: `S` is authority, and nothing
//! here has entered the accepted pipeline. The remaining branches §6 names (deep
//! PPM as a distribution provider, the temporal learned residual expert) are
//! where the headroom is; this module exists so a future roster can be measured
//! against the flat baseline without rewriting the mixing topology.

use super::{squash, Mixer, StretchTable};

/// One family of inputs: a named, contiguous range `[start, start + len)` of the
/// caller's shared input roster. Families must partition the roster (first
/// `start == 0`, each `start == previous start + previous len`) — a hole or an
/// overlap is a mis-declaration, not a roster, and is rejected rather than
/// silently mis-indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Family {
    pub name: &'static str,
    pub start: usize,
    pub len: usize,
}

/// Why a family roster was refused. Every variant names the offending family so
/// a caller learns which declaration to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HierError {
    /// No families at all: there is nothing to mix.
    NoFamilies,
    /// A family with no inputs cannot have a local mixer and cannot contribute;
    /// it is refused instead of aliasing an empty input range.
    EmptyFamily { name: &'static str },
    /// The roster is not a contiguous partition starting at zero.
    NotContiguous {
        name: &'static str,
        start: usize,
        expected: usize,
    },
    /// `start + len` overflowed `usize`.
    TooLarge,
}

impl std::fmt::Display for HierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HierError::NoFamilies => write!(f, "a hierarchy needs at least one family"),
            HierError::EmptyFamily { name } => {
                write!(f, "family {name:?} has zero inputs")
            }
            HierError::NotContiguous {
                name,
                start,
                expected,
            } => write!(
                f,
                "family {name:?} starts at {start}, expected {expected}; families must form a contiguous partition"
            ),
            HierError::TooLarge => write!(f, "family range overflows usize"),
        }
    }
}

impl std::error::Error for HierError {}

/// Per-family instrumentation, always recorded (the measurement *is* the point of
/// §14.33). Every field is a diagnostic: none of it is coded, and a caller prints
/// it with [`FamilyStats::render`].
#[derive(Debug, Clone, PartialEq)]
pub struct FamilyStats {
    pub name: &'static str,
    pub start: usize,
    pub len: usize,
    /// Bits observed while this family was part of the top mix. The denominator
    /// for every rate below; identical for every family in a run.
    pub total_bits: u64,
    /// Bits at which at least one input in the family was non-zero. A family that
    /// is never active cannot be contributing, and this makes that visible.
    pub active_bits: u64,
    /// Ideal log loss (bits) of the family's **local** mixer output alone. If the
    /// local mixer is no better than a coin, this is `total_bits`.
    pub local_bits: f64,
    /// Signed marginal contribution to the top-level ideal loss: the loss with
    /// the family's top input removed minus the actual loss, summed over bits.
    /// Positive means removing the family would cost more, i.e. it helps.
    /// `≈ 0` means the family adds nothing — the honest signal §14.33 requires.
    pub marginal_bits: f64,
    /// Top-level weight assigned to this family, summed over updates (16.16).
    pub top_weight_sum: f64,
    pub top_weight_n: u64,
    pub top_weight_min: i32,
    pub top_weight_max: i32,
    /// Local-mixer weight distribution across the family's experts.
    pub local_weight_sum: f64,
    pub local_weight_n: u64,
    pub local_weight_min: i32,
    pub local_weight_max: i32,
}

impl FamilyStats {
    fn new(f: &Family) -> Self {
        FamilyStats {
            name: f.name,
            start: f.start,
            len: f.len,
            total_bits: 0,
            active_bits: 0,
            local_bits: 0.0,
            marginal_bits: 0.0,
            top_weight_sum: 0.0,
            top_weight_n: 0,
            top_weight_min: i32::MAX,
            top_weight_max: i32::MIN,
            local_weight_sum: 0.0,
            local_weight_n: 0,
            local_weight_min: i32::MAX,
            local_weight_max: i32::MIN,
        }
    }

    /// Fraction of bits at which the family had any active input.
    pub fn activation_rate(&self) -> f64 {
        if self.total_bits == 0 {
            0.0
        } else {
            self.active_bits as f64 / self.total_bits as f64
        }
    }

    /// Mean local-mixer output loss per active bit (bits).
    pub fn local_bits_per_bit(&self) -> f64 {
        if self.total_bits == 0 {
            0.0
        } else {
            self.local_bits / self.total_bits as f64
        }
    }

    /// Mean marginal contribution per bit (bits). Positive = helps.
    pub fn marginal_per_bit(&self) -> f64 {
        if self.total_bits == 0 {
            0.0
        } else {
            self.marginal_bits / self.total_bits as f64
        }
    }

    /// Mean top-level weight for this family (16.16).
    pub fn top_weight_mean(&self) -> f64 {
        if self.top_weight_n == 0 {
            0.0
        } else {
            self.top_weight_sum / self.top_weight_n as f64
        }
    }

    /// Mean local-mixer weight (16.16).
    pub fn local_weight_mean(&self) -> f64 {
        if self.local_weight_n == 0 {
            0.0
        } else {
            self.local_weight_sum / self.local_weight_n as f64
        }
    }

    pub fn render(&self) -> String {
        format!(
            "{:<14} in[{:>3}..{:<3}) act {:>6.2}%  local {:>8.4} b/bit  marginal {:>+9.4} b/bit  top w [{:>8}..{:<8}] mean {:>10.1}  local w [{:>8}..{:<8}] mean {:>10.1}",
            self.name,
            self.start,
            self.start + self.len,
            self.activation_rate() * 100.0,
            self.local_bits_per_bit(),
            self.marginal_per_bit(),
            if self.top_weight_n == 0 { 0 } else { self.top_weight_min },
            if self.top_weight_n == 0 { 0 } else { self.top_weight_max },
            self.top_weight_mean(),
            if self.local_weight_n == 0 { 0 } else { self.local_weight_min },
            if self.local_weight_n == 0 { 0 } else { self.local_weight_max },
            self.local_weight_mean(),
        )
    }
}

/// A two-level mixer. Each [`Family`] gets a [`Mixer`] over just its input range
/// (one weight set: the local problem is *which of my experts to trust*, which is
/// largely context-free); the top [`Mixer`] mixes the stretched family outputs
/// under a caller-supplied context. Integers only in the mixing path — the float
/// appears solely in the instrumentation.
pub struct HierMixer {
    families: Vec<Family>,
    locals: Vec<Mixer>,
    top: Mixer,
    n_inputs: usize,
    n_families: usize,
    top_n_ctx: usize,
    st: StretchTable,
    /// Cached per-`mix` state, so `update` can price the prediction it just made.
    inputs: Vec<i32>,
    fam_out: Vec<i32>,
    fam_st: Vec<i32>,
    top_cx: usize,
    top_dot: i64,
    top_pr: i32,
    instrument: bool,
    stats: Vec<FamilyStats>,
}

impl HierMixer {
    /// Build a hierarchy over a contiguous partition of an `n`-input roster.
    /// `top_n_ctx` is the number of weight sets the top mixer selects between;
    /// `lr` is the fixed-point learning rate applied to every mixer (16 == 1.0),
    /// so a caller can splice the hierarchy anywhere the flat mixer already sits
    /// and reuse the existing `mixer_lr` plumbing.
    pub fn new(families: &[Family], top_n_ctx: usize, lr: i32) -> Result<Self, HierError> {
        if families.is_empty() {
            return Err(HierError::NoFamilies);
        }
        let mut expected = 0usize;
        for f in families {
            if f.len == 0 {
                return Err(HierError::EmptyFamily { name: f.name });
            }
            if f.start != expected {
                return Err(HierError::NotContiguous {
                    name: f.name,
                    start: f.start,
                    expected,
                });
            }
            expected = expected.checked_add(f.len).ok_or(HierError::TooLarge)?;
        }
        let n_inputs = expected;
        let n_families = families.len();
        let mut locals: Vec<Mixer> = families.iter().map(|f| Mixer::new(f.len, 1)).collect();
        for l in &mut locals {
            l.set_learning_rate(lr);
        }
        let mut top = Mixer::new(n_families, top_n_ctx.max(1));
        top.set_learning_rate(lr);
        Ok(HierMixer {
            families: families.to_vec(),
            locals,
            top,
            n_inputs,
            n_families,
            top_n_ctx: top_n_ctx.max(1),
            st: StretchTable::new(),
            inputs: vec![0; n_inputs],
            fam_out: vec![2048; n_families],
            fam_st: vec![0; n_families],
            top_cx: 0,
            top_dot: 0,
            top_pr: 2048,
            instrument: true,
            stats: families.iter().map(FamilyStats::new).collect(),
        })
    }

    pub fn set_learning_rate(&mut self, lr: i32) {
        for l in &mut self.locals {
            l.set_learning_rate(lr);
        }
        self.top.set_learning_rate(lr);
    }

    /// Turn the float/log instrumentation off for throughput. With it off the
    /// stats stop advancing; it is on by default because measuring families is
    /// what this module is for.
    pub fn set_instrument(&mut self, on: bool) {
        self.instrument = on;
    }

    pub fn input_count(&self) -> usize {
        self.n_inputs
    }

    pub fn family_count(&self) -> usize {
        self.n_families
    }

    pub fn top_context_count(&self) -> usize {
        self.top_n_ctx
    }

    pub fn families(&self) -> &[Family] {
        &self.families
    }

    pub fn stats(&self) -> &[FamilyStats] {
        &self.stats
    }

    /// Mix all `inputs` (stretched to `[-2047, 2047]`, in roster order) and return
    /// the composite 12-bit probability, clamped to `1..=4094` so no downstream
    /// coder can be handed a certainty.
    ///
    /// `ctx` selects the top-level weight set exactly as the flat [`Mixer`] does:
    /// `ctx as usize % top_n_ctx`. It must be a function of data already coded
    /// (the partial byte `c0`, a structural class, a word-state flag, a bucket of
    /// expert disagreement …). A caller with a `signal::SignalState` can hash the
    /// fields it needs into this `u32`; this module stays independent of that
    /// feature.
    pub fn mix(&mut self, inputs: &[i32], ctx: u32) -> i32 {
        assert_eq!(
            inputs.len(),
            self.n_inputs,
            "hier mixer got {} inputs, expected {}",
            inputs.len(),
            self.n_inputs
        );
        self.inputs.copy_from_slice(inputs);
        for fi in 0..self.n_families {
            let start = self.families[fi].start;
            let len = self.families[fi].len;
            let p = self.locals[fi].mix(&inputs[start..start + len], 0);
            self.fam_out[fi] = p;
            self.fam_st[fi] = self.st.stretch(p);
        }
        self.top_cx = (ctx as usize) % self.top_n_ctx;
        let pr = self.top.mix(&self.fam_st, ctx as usize);
        // Recompute the top logit so `update` can price marginal contributions by
        // removing one family's stretched input. This is the same dot product the
        // flat mixer performs, kept here because `Mixer` does not expose it.
        let base = self.top_cx * self.n_families;
        let mut dot: i64 = 0;
        for i in 0..self.n_families {
            dot += (self.top.weights[base + i] as i64) * (self.fam_st[i] as i64);
        }
        self.top_dot = dot;
        self.top_pr = pr;
        pr.clamp(1, 4094)
    }

    /// Update every mixer and record the per-family instrumentation for the bit
    /// just coded. Marginals are computed against the weights that *made* the
    /// prediction, before they move.
    #[inline]
    pub fn update(&mut self, y: u32) {
        let yb = y & 1;
        if self.instrument {
            for fi in 0..self.n_families {
                let start = self.families[fi].start;
                let len = self.families[fi].len;
                let active = self.inputs[start..start + len].iter().any(|&v| v != 0);
                let local_pr = self.fam_out[fi];
                let fam_st = self.fam_st[fi];
                let w = self.top.weights[self.top_cx * self.n_families + fi];
                let top_pr = self.top_pr;
                // Counterfactual: the top logit with this family's input zeroed.
                let dot_f = self.top_dot - (w as i64) * (fam_st as i64);
                let marg_pr = squash(((dot_f >> 16) as i32).clamp(-2047, 2047));
                let mut lmin = i32::MAX;
                let mut lmax = i32::MIN;
                let mut lsum = 0.0f64;
                let mut ln = 0u64;
                for &v in self.locals[fi].weights.iter() {
                    lmin = lmin.min(v);
                    lmax = lmax.max(v);
                    lsum += v as f64;
                    ln += 1;
                }
                let s = &mut self.stats[fi];
                s.total_bits += 1;
                if active {
                    s.active_bits += 1;
                }
                s.local_bits += ideal_bits(local_pr, yb);
                s.top_weight_sum += w as f64;
                s.top_weight_n += 1;
                s.top_weight_min = s.top_weight_min.min(w);
                s.top_weight_max = s.top_weight_max.max(w);
                s.marginal_bits += ideal_bits(marg_pr, yb) - ideal_bits(top_pr, yb);
                s.local_weight_min = s.local_weight_min.min(lmin);
                s.local_weight_max = s.local_weight_max.max(lmax);
                s.local_weight_sum += lsum;
                s.local_weight_n += ln;
            }
        }
        for l in &mut self.locals {
            l.update(y);
        }
        self.top.update(y);
    }

    /// Total weight storage in bytes: one weight set per family plus the
    /// context-selected top sets.
    pub fn memory_bytes(&self) -> u64 {
        ((self.n_inputs + self.n_families * self.top_n_ctx) as u64) * 4
    }

    /// Print every family's instrumentation, one line each.
    pub fn render_stats(&self) -> String {
        let mut out = String::new();
        for s in &self.stats {
            out.push_str(&s.render());
            out.push('\n');
        }
        out
    }
}

/// Ideal code length of one bit, in bits: `-log2 P(observed)`. Diagnostics only;
/// this is the one place a float is allowed to appear.
fn ideal_bits(pr: i32, y: u32) -> f64 {
    let p = (pr.clamp(1, 4095) as f64) / 4096.0;
    let py = if y != 0 { p } else { 1.0 - p };
    -py.log2()
}

// ---------------------------------------------------------------------------
// The measured comparison against the flat mixer (§14.33 requirement 3).
//
// The experts are a fixed, in-file roster reusing the production expert types
// (`ContextModel`, `MatchModel`) so the topology — not the expert quality — is
// what changes between the two mixers. Both mixers see the *same* 16 stretched
// inputs at every bit; only how those inputs are combined differs.
// ---------------------------------------------------------------------------

const ORDERS: [usize; 6] = [0, 1, 2, 3, 4, 6];
const N_CTX_MODELS: usize = 14;
/// Context models: two words per slot.
const MODEL_BITS: u32 = 16;
/// Match tables: one `u32` per slot.
const MATCH_BITS: u32 = 16;

/// The roster's family partition. The same declaration drives both the report and
/// any future caller: order/byte models, word/lexical models, structural-state
/// models, match predictors, and high-order distribution-provider models.
pub const FAMILIES: [Family; 5] = [
    Family {
        name: "order/byte",
        start: 0,
        len: 6,
    },
    Family {
        name: "word/lexical",
        start: 6,
        len: 3,
    },
    Family {
        name: "structural",
        start: 9,
        len: 3,
    },
    Family {
        name: "match",
        start: 12,
        len: 2,
    },
    Family {
        name: "ppm",
        start: 14,
        len: 2,
    },
];

/// Total inputs in [`FAMILIES`].
pub const N_INPUTS: usize = 16;

/// The comparison report. `usize`/`u64` fields are exact; the `f64` fields are the
/// diagnostic ideal lengths §14.33 asks for and are never a claim about `S`.
#[derive(Debug, Clone, PartialEq)]
pub struct CompareReport {
    pub bytes: usize,
    pub n_inputs: usize,
    pub n_families: usize,
    pub flat_n_ctx: usize,
    pub hier_top_n_ctx: usize,
    /// Ideal codelength of the flat `Mixer` fed all inputs in one competition.
    pub flat_ideal_bits: f64,
    /// Ideal codelength of [`HierMixer`] over the same inputs, grouped.
    pub hier_ideal_bits: f64,
    /// Weight storage of the flat mixer.
    pub flat_mixer_bytes: u64,
    /// Weight storage of the hierarchy (locals + top).
    pub hier_mixer_bytes: u64,
    /// Expert-table storage, identical for both (reported so the comparison is
    /// complete, not to flatter either side).
    pub expert_bytes: u64,
    pub per_family: Vec<FamilyStats>,
}

impl CompareReport {
    pub fn flat_bpc(&self) -> f64 {
        self.flat_ideal_bits / self.bytes.max(1) as f64
    }

    pub fn hier_bpc(&self) -> f64 {
        self.hier_ideal_bits / self.bytes.max(1) as f64
    }

    /// `hier - flat`: negative means the hierarchy codes cheaper.
    pub fn delta_bits(&self) -> f64 {
        self.hier_ideal_bits - self.flat_ideal_bits
    }

    pub fn hierarchy_wins(&self) -> bool {
        self.hier_ideal_bits < self.flat_ideal_bits
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "flat vs hier on {} bytes, {} inputs in {} families\n",
            self.bytes, self.n_inputs, self.n_families
        ));
        out.push_str(&format!(
            "  flat  ctx={:<6} weights={:>9} B  ideal={:>12.1} bits ({:.4} bpc)\n",
            self.flat_n_ctx,
            self.flat_mixer_bytes,
            self.flat_ideal_bits,
            self.flat_bpc()
        ));
        out.push_str(&format!(
            "  hier  top_ctx={:<6} weights={:>9} B  ideal={:>12.1} bits ({:.4} bpc)\n",
            self.hier_top_n_ctx,
            self.hier_mixer_bytes,
            self.hier_ideal_bits,
            self.hier_bpc()
        ));
        out.push_str(&format!(
            "  expert tables (shared) = {} B\n",
            self.expert_bytes
        ));
        out.push_str(&format!(
            "  delta = {:+.1} bits ({:+.4} bpc); hierarchy {} on this slice\n",
            self.delta_bits(),
            self.delta_bits() / self.bytes.max(1) as f64,
            if self.hierarchy_wins() {
                "WINS (ideal only; not an adoption)"
            } else {
                "LOSES (ideal only)"
            }
        ));
        out.push_str("  per family:\n");
        for s in &self.per_family {
            out.push_str("    ");
            out.push_str(&s.render());
            out.push('\n');
        }
        out
    }
}

const HASH_C: u64 = 0x9E37_79B9_7F4A_7C15;

#[inline]
fn mix32(a: u32, b: u32) -> u32 {
    let mut h = (a as u64).wrapping_mul(0x9E37_79B1) ^ b as u64;
    h ^= h >> 29;
    (h.wrapping_mul(HASH_C) >> 32) as u32
}

#[inline]
fn byte_class(b: u8) -> u32 {
    match b {
        b'a'..=b'z' | b'A'..=b'Z' => 0,
        b'0'..=b'9' => 1,
        b' ' | b'\t' => 2,
        b'\n' | b'\r' => 3,
        b'{' | b'}' | b'[' | b']' | b'<' | b'>' | b'|' | b'=' | b'&' | b'/' => 4,
        _ => 5,
    }
}

/// The in-file expert roster. It reuses `ContextModel` and `MatchModel` so the
/// comparison isolates the mixing topology, and it keeps the byte history the
/// context keys need.
struct Harness {
    ctx_models: Vec<crate::context::ContextModel>,
    matches: Vec<crate::context::MatchModel>,
    st: StretchTable,
    buf: Vec<u8>,
    inputs: Vec<i32>,
    ctx_of: [u32; N_CTX_MODELS],
    bit_pos: u32,
    last_byte: u8,
    prev_byte: u8,
    word_hash: u64,
    prev_word: u64,
    word_bytes: Vec<u8>,
    line_pos: usize,
    run_class: u32,
    run_len: usize,
    brace: i32,
    bracket: i32,
}

impl Harness {
    fn new() -> Self {
        let ctx_models = (0..N_CTX_MODELS)
            .map(|k| crate::context::ContextModel::new(MODEL_BITS, if k < 6 { 4 } else { 5 }))
            .collect();
        let matches = vec![
            crate::context::MatchModel::new(MATCH_BITS, 6, 0, false),
            crate::context::MatchModel::new(MATCH_BITS, 12, 0, true),
        ];
        Harness {
            ctx_models,
            matches,
            st: StretchTable::new(),
            buf: Vec::new(),
            inputs: vec![0; N_INPUTS],
            ctx_of: [0; N_CTX_MODELS],
            bit_pos: 0,
            last_byte: 0,
            prev_byte: 0,
            word_hash: 0,
            prev_word: 0,
            word_bytes: Vec::new(),
            line_pos: 0,
            run_class: 0,
            run_len: 0,
            brace: 0,
            bracket: 0,
        }
    }

    /// Prime the match tiers on the empty buffer, as `Predictor::prime` does.
    fn prime(&mut self) {
        for mm in &mut self.matches {
            mm.begin_byte(&self.buf);
        }
    }

    /// Compute every model's context for the byte about to be coded. All keys are
    /// functions of bytes already in `buf`, so the decoder can reproduce them.
    fn begin_byte(&mut self) {
        let n = self.buf.len();
        for (k, &ord) in ORDERS.iter().enumerate() {
            let take = ord.min(n);
            self.ctx_of[k] = crate::context::hash_bytes(&self.buf[n - take..]);
        }
        let w = (self.word_hash ^ (self.word_hash >> 32)) as u32;
        self.ctx_of[6] = mix32(w, self.word_bytes.len() as u32);
        let stem = if self.word_bytes.is_empty() {
            0
        } else {
            crate::context::hash_bytes(crate::context::stem_bytes(&self.word_bytes))
        };
        self.ctx_of[7] = mix32(stem, (self.word_hash != 0) as u32);
        self.ctx_of[8] = mix32((self.prev_word ^ (self.prev_word >> 32)) as u32, w);
        self.ctx_of[9] = mix32(
            mix32(self.line_pos as u32, byte_class(self.last_byte)),
            self.last_byte as u32,
        );
        self.ctx_of[10] = mix32(
            (self.run_class << 8) | self.run_len as u32,
            (n & 0xff) as u32,
        );
        self.ctx_of[11] = mix32(
            ((self.brace as u32) << 4) | (self.bracket as u32),
            byte_class(self.last_byte),
        );
        let take = 8.min(n);
        self.ctx_of[12] = crate::context::hash_bytes(&self.buf[n - take..]);
        let mut key = [0u8; 3];
        for (j, off) in [1usize, 3, 5].iter().enumerate() {
            key[j] = if n >= *off { self.buf[n - *off] } else { 0 };
        }
        self.ctx_of[13] = crate::context::hash_bytes(&key);
        for k in 0..N_CTX_MODELS {
            self.ctx_models[k].set_context(self.ctx_of[k]);
        }
    }

    /// Produce the 16 stretched expert predictions for the partial byte `c0`.
    fn predict_inputs(&mut self, c0: u32) {
        for k in 0..12 {
            self.inputs[k] = self.ctx_models[k].predict(c0, &self.st, 32768);
        }
        self.inputs[12] = self.matches[0].predict(self.bit_pos);
        self.inputs[13] = self.matches[1].predict(self.bit_pos);
        self.inputs[14] = self.ctx_models[12].predict(c0, &self.st, 32768);
        self.inputs[15] = self.ctx_models[13].predict(c0, &self.st, 32768);
    }

    fn update_models(&mut self, y: u32) {
        for k in 0..N_CTX_MODELS {
            self.ctx_models[k].update(y);
        }
        for mm in &mut self.matches {
            mm.update(y);
        }
    }

    /// Advance the byte-level state after a byte has been coded: word tracking,
    /// line/run/depth state, and the match tiers, mirroring `Predictor::obs_core`.
    fn observe_byte(&mut self, b: u8) {
        self.prev_byte = self.last_byte;
        self.last_byte = b;
        let lc = b.to_ascii_lowercase();
        if lc.is_ascii_alphabetic() || lc == b'\'' {
            self.word_hash = self.word_hash.wrapping_mul(HASH_C) ^ (lc as u64 | 0x100);
            if self.word_bytes.len() < 64 {
                self.word_bytes.push(lc);
            }
        } else {
            if self.word_hash != 0 {
                self.prev_word = self.word_hash;
            }
            self.word_hash = 0;
            self.word_bytes.clear();
        }
        if b == b'\n' {
            self.line_pos = 0;
        } else {
            self.line_pos = (self.line_pos + 1).min(63);
        }
        let cls = byte_class(b);
        if cls == self.run_class {
            self.run_len = (self.run_len + 1).min(15);
        } else {
            self.run_class = cls;
            self.run_len = 1;
        }
        match b {
            b'{' => self.brace += 1,
            b'}' => self.brace -= 1,
            b'[' => self.bracket += 1,
            b']' => self.bracket -= 1,
            _ => {}
        }
        self.brace = self.brace.clamp(0, 8);
        self.bracket = self.bracket.clamp(0, 8);
        self.buf.push(b);
        for mm in &mut self.matches {
            let _ = mm.byte_boundary(&self.buf);
            mm.begin_byte(&self.buf);
        }
    }

    fn memory_bytes(&self) -> u64 {
        let mut m = self.buf.capacity() as u64;
        for c in &self.ctx_models {
            m += c.memory_bytes();
        }
        for mm in &self.matches {
            m += mm.memory_bytes();
        }
        m
    }
}

/// Measure the flat mixer against the hierarchy on `data`, at the same total
/// input count and with weight memory matched to within one weight set.
///
/// `flat_n_ctx` fixes the flat mixer's context count; the hierarchy's top context
/// count is derived so `n_inputs + n_families * top_n_ctx ≈ n_inputs * flat_n_ctx`
/// (its local mixers hold one weight set each, which is the point of the design).
/// Both see identical inputs on every bit.
pub fn compare_flat_vs_hier(data: &[u8], flat_n_ctx: usize) -> CompareReport {
    let flat_n_ctx = flat_n_ctx.max(1);
    let n_families = FAMILIES.len();
    let top_n_ctx = ((flat_n_ctx * N_INPUTS) / n_families).max(1);

    let mut h = Harness::new();
    let mut flat = Mixer::new(N_INPUTS, flat_n_ctx);
    flat.set_learning_rate(12);
    let mut hier = HierMixer::new(&FAMILIES, top_n_ctx, 12).expect("FAMILIES is a partition");
    h.prime();

    let mut flat_bits = 0.0f64;
    let mut hier_bits = 0.0f64;
    for &b in data {
        h.begin_byte();
        let mut c0 = 1u32;
        for bit_pos in 0..8 {
            h.bit_pos = bit_pos;
            h.predict_inputs(c0);
            // Identical causal context for both mixers: partial byte, last byte,
            // byte before that.
            let ctx =
                (c0 as usize) | ((h.last_byte as usize) << 9) | ((h.prev_byte as usize) << 17);
            let pf = flat.mix(&h.inputs, ctx);
            let ph = hier.mix(&h.inputs, ctx as u32);
            let y = ((b >> (7 - bit_pos)) & 1) as u32;
            flat_bits += ideal_bits(pf, y);
            hier_bits += ideal_bits(ph, y);
            flat.update(y);
            hier.update(y);
            h.update_models(y);
            c0 = (c0 << 1) | y;
        }
        h.observe_byte(b);
    }

    CompareReport {
        bytes: data.len(),
        n_inputs: N_INPUTS,
        n_families,
        flat_n_ctx,
        hier_top_n_ctx: top_n_ctx,
        flat_ideal_bits: flat_bits,
        hier_ideal_bits: hier_bits,
        flat_mixer_bytes: (N_INPUTS * flat_n_ctx * 4) as u64,
        hier_mixer_bytes: hier.memory_bytes(),
        expert_bytes: h.memory_bytes(),
        per_family: hier.stats().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: [Family; 2] = [
        Family {
            name: "a",
            start: 0,
            len: 1,
        },
        Family {
            name: "b",
            start: 1,
            len: 1,
        },
    ];

    /// Deterministic pseudo-corpus with wiki-like structure, for tests that must
    /// not depend on `evidence/`.
    fn sample_corpus(n: usize) -> Vec<u8> {
        const WORDS: [&str; 16] = [
            "the", "of", "and", "in", "wiki", "page", "title", "link", "template", "history",
            "world", "music", "river", "city", "born", "is",
        ];
        let mut out = Vec::with_capacity(n + 64);
        let mut s: u64 = 0x1234_5678_9abc_def0;
        while out.len() < n {
            out.extend_from_slice(b"<page>\n");
            let k = 4 + (next_rand(&mut s) % 12) as usize;
            for _ in 0..k {
                out.extend_from_slice(WORDS[(next_rand(&mut s) % 16) as usize].as_bytes());
                out.push(b' ');
            }
            out.extend_from_slice(b"{{cite|id=1}} [[Link]] 123\n</page>\n");
        }
        out.truncate(n);
        out
    }

    fn next_rand(s: &mut u64) -> u64 {
        let mut x = *s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *s = x;
        x
    }

    #[test]
    fn determinism_across_runs() {
        let inputs = [1200, -300, 0, 700];
        let fams = [
            Family {
                name: "x",
                start: 0,
                len: 2,
            },
            Family {
                name: "y",
                start: 2,
                len: 2,
            },
        ];
        let mut a = HierMixer::new(&fams, 4, 12).unwrap();
        let mut b = HierMixer::new(&fams, 4, 12).unwrap();
        assert_eq!(a.stats().len(), 2);
        for i in 0..500 {
            let cx = (i % 4) as u32;
            let pa = a.mix(&inputs, cx);
            let pb = b.mix(&inputs, cx);
            assert_eq!(pa, pb, "mix diverged at {i}");
            let y = (i % 3 == 0) as u32;
            a.update(y);
            b.update(y);
        }
        // Same weights, same recorded statistics.
        assert_eq!(a.top.weights, b.top.weights);
        assert_eq!(a.stats(), b.stats());
    }

    #[test]
    fn probabilities_stay_in_the_open_interval() {
        let fams = [
            Family {
                name: "x",
                start: 0,
                len: 2,
            },
            Family {
                name: "y",
                start: 2,
                len: 2,
            },
        ];
        let mut h = HierMixer::new(&fams, 8, 12).unwrap();
        // Drive the inputs to the extremes so squash would return 0/4095.
        let mut s: u64 = 7;
        for i in 0..20_000u32 {
            let r = next_rand(&mut s);
            let inputs = [
                if r & 1 == 0 { -2047 } else { 2047 },
                if r & 2 == 0 { -2047 } else { 2047 },
                if r & 4 == 0 { -2047 } else { 2047 },
                if r & 8 == 0 { -2047 } else { 2047 },
            ];
            let p = h.mix(&inputs, (i % 8) as u32);
            assert!((1..4095).contains(&p), "p={p} out of 1..4095 at {i}");
            h.update((r >> 5) as u32 & 1);
        }
    }

    #[test]
    fn top_context_changes_the_weights_and_the_output() {
        let fams = [
            Family {
                name: "x",
                start: 0,
                len: 1,
            },
            Family {
                name: "y",
                start: 1,
                len: 1,
            },
        ];
        let mut h = HierMixer::new(&fams, 2, 12).unwrap();
        let inputs = [1500, -1200];
        // Train only context 0 toward y = 1.
        for _ in 0..5_000 {
            h.mix(&inputs, 0);
            h.update(1);
        }
        let p0 = h.mix(&inputs, 0);
        let p1 = h.mix(&inputs, 1);
        assert_ne!(p0, p1, "context 1 must not inherit context 0's training");
        assert!(p0 > p1, "context 0 was trained toward 1: {p0} vs {p1}");
        // The two selected weight vectors must actually differ.
        let n = h.n_families;
        assert_ne!(
            &h.top.weights[0..n],
            &h.top.weights[n..2 * n],
            "per-context weight sets did not diverge"
        );
    }

    #[test]
    fn zero_input_family_is_rejected() {
        let bad = [
            Family {
                name: "ok",
                start: 0,
                len: 2,
            },
            Family {
                name: "empty",
                start: 2,
                len: 0,
            },
        ];
        assert_eq!(
            HierMixer::new(&bad, 4, 12).err(),
            Some(HierError::EmptyFamily { name: "empty" })
        );
        assert_eq!(
            HierMixer::new(&[], 4, 12).err(),
            Some(HierError::NoFamilies)
        );

        let gap = [
            Family {
                name: "a",
                start: 0,
                len: 1,
            },
            Family {
                name: "b",
                start: 2,
                len: 1,
            },
        ];
        assert_eq!(
            HierMixer::new(&gap, 4, 12).err(),
            Some(HierError::NotContiguous {
                name: "b",
                start: 2,
                expected: 1
            })
        );

        let overlap = [
            Family {
                name: "a",
                start: 0,
                len: 2,
            },
            Family {
                name: "b",
                start: 1,
                len: 1,
            },
        ];
        assert!(matches!(
            HierMixer::new(&overlap, 4, 12),
            Err(HierError::NotContiguous { .. })
        ));
    }

    #[test]
    fn marginals_match_a_hand_computed_two_family_case() {
        // One bit, one expert per family, no context: every weight is at its
        // initial value, so the whole arithmetic is reproducible from first
        // principles.
        let mut h = HierMixer::new(&TWO, 1, 12).unwrap();
        let st = StretchTable::new();
        let st_a = 900;
        let st_b = -400;
        let inputs = [st_a, st_b];
        let out = h.mix(&inputs, 0);
        // Capture the weights that *made* this prediction, before they move.
        let top_w0 = h.top.weights.clone();
        let loc_w0 = h.locals[0].weights.clone();
        let y = 1u32;
        h.update(y);

        // Local mixers: n = 1, init weight 65536, so d = 65536 * st >> 16 = st.
        let pr_a = squash(st_a);
        let pr_b = squash(st_b);
        let fam_st_a = st.stretch(pr_a);
        let fam_st_b = st.stretch(pr_b);
        // Top mixer: n = 2, init weight 65536 / 2 = 32768.
        let w = 32768i64;
        let dot = w * fam_st_a as i64 + w * fam_st_b as i64;
        let top_pr = squash(((dot >> 16) as i32).clamp(-2047, 2047));
        assert_eq!(out, top_pr.clamp(1, 4094));
        assert_eq!(top_w0, vec![32768, 32768], "top init weights");
        assert_eq!(loc_w0, vec![65536], "local init weight");

        // Family a.
        let marg_a = squash((((dot - w * fam_st_a as i64) >> 16) as i32).clamp(-2047, 2047));
        let s = &h.stats()[0];
        assert_eq!(s.total_bits, 1);
        assert_eq!(s.active_bits, 1);
        assert!((s.local_bits - ideal_bits(pr_a, y)).abs() < 1e-12);
        assert!((s.marginal_bits - (ideal_bits(marg_a, y) - ideal_bits(top_pr, y))).abs() < 1e-12);
        assert_eq!(s.top_weight_min, 32768);
        assert_eq!(s.top_weight_max, 32768);

        // Family b.
        let marg_b = squash((((dot - w * fam_st_b as i64) >> 16) as i32).clamp(-2047, 2047));
        let s = &h.stats()[1];
        assert_eq!(s.total_bits, 1);
        assert!((s.local_bits - ideal_bits(pr_b, y)).abs() < 1e-12);
        assert!((s.marginal_bits - (ideal_bits(marg_b, y) - ideal_bits(top_pr, y))).abs() < 1e-12);
    }

    #[test]
    fn a_silent_family_reports_zero_marginal() {
        // Every input is zero: no family has anything to say, each local mixer is
        // a coin, and every family's marginal contribution must be ~0.
        let mut h = HierMixer::new(&TWO, 4, 12).unwrap();
        let inputs = [0, 0];
        for i in 0..2_000 {
            h.mix(&inputs, (i % 4) as u32);
            h.update((i % 2) as u32);
        }
        for s in h.stats() {
            assert_eq!(s.active_bits, 0, "{} claimed activation", s.name);
            assert!(
                s.marginal_per_bit().abs() < 1e-9,
                "{} marginal = {}",
                s.name,
                s.marginal_per_bit()
            );
            // A mixer of zero inputs predicts `squash(0) = 2047`, i.e. a hair
            // worse than 1 bit/bit — effectively a coin.
            assert!((s.local_bits_per_bit() - 1.0).abs() < 1e-2);
        }
    }

    #[test]
    fn report_reproduces_on_a_second_call() {
        let data = sample_corpus(16 * 1024);
        let a = compare_flat_vs_hier(&data, 1024);
        let b = compare_flat_vs_hier(&data, 1024);
        assert_eq!(a, b);
        assert_eq!(a.n_inputs, N_INPUTS);
        assert_eq!(a.n_families, FAMILIES.len());
        assert_eq!(a.flat_mixer_bytes, 65_536);
        // "Comparable memory": within one weight set of the flat mixer.
        let diff = (a.hier_mixer_bytes as i64 - a.flat_mixer_bytes as i64).abs();
        assert!(diff <= N_INPUTS as i64 * 4, "memory not comparable: {a:?}");
        // The report is complete and honest about direction, whatever it is.
        let _ = a.hierarchy_wins();
        assert!(a.expert_bytes > 0);
    }

    #[test]
    fn every_family_is_represented_and_named() {
        let data = sample_corpus(8 * 1024);
        let r = compare_flat_vs_hier(&data, 512);
        let names: Vec<&str> = r.per_family.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            vec!["order/byte", "word/lexical", "structural", "match", "ppm"]
        );
        // Every family saw every bit.
        for s in &r.per_family {
            assert_eq!(s.total_bits, data.len() as u64 * 8);
        }
        // Instrumentation prints without panicking.
        assert!(r.render().contains("per family"));
    }

    #[test]
    #[ignore = "measures enwik6; run deliberately"]
    fn enwik6_report() {
        let path = "evidence/corpus/enwik6";
        if !std::path::Path::new(path).exists() {
            eprintln!("enwik6_report: {path} not present; skipping");
            return;
        }
        let data = std::fs::read(path).expect("read enwik6");
        let slice = &data[..data.len().min(1 << 20)];
        let r = compare_flat_vs_hier(slice, 1024);
        println!("{}", r.render());
    }
}
