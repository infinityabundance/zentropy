//! Context models, a match model, and the composite bitwise predictor.
//!
//! The predictor codes each byte MSB-first as eight binary decisions. Every
//! expert produces a stretched prediction; the mixer combines them; calibration
//! stages refine the composite; the range coder codes the bit.
//!
//! Experts are explicit and individually ablatable. That matters because the
//! admission rule is `ΔS < 0` (§0, law 7): an expert that does not pay for its
//! own model bytes and runtime is deleted, regardless of pedigree. The
//! [`ModelSpec`] list is the ablation surface.
//!
//! Contexts are deliberately plain direct models rather than exotic bit-history
//! structures: they are cheap to reason about, cheap in binary size, and give
//! every richer mechanism a fair baseline to beat.

use crate::mixer::{Apm, Mixer, StretchTable};

/// Fixed-point multiplier used to decorrelate context and partial byte.
const MIX_C: u32 = 0x9E37_79B1;
/// Fixed-point multiplier for hashing context bytes.
const HASH_C: u64 = 0x9E37_79B9_7F4A_7C15;

/// Hash a byte string into a 32-bit context id.
#[inline]
pub fn hash_bytes(bytes: &[u8]) -> u32 {
    let mut h: u64 = HASH_C;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(HASH_C);
    }
    (h >> 32) as u32
}

/// Combine a running word hash with one byte.
#[inline]
fn word_add(h: u64, b: u8) -> u64 {
    h.wrapping_mul(HASH_C) ^ (b as u64 | 0x100)
}

/// Hash of a whole lowercase word, using the same fold as the running `word_cur`.
#[cfg(feature = "word-class")]
pub fn word_hash(w: &[u8]) -> u64 {
    let mut h = 0u64;
    for &b in w {
        h = word_add(h, b.to_ascii_lowercase());
    }
    h
}

/// Sorted `(word_hash, class)` table for the closed-class function words.
#[cfg(feature = "word-class")]
pub fn fnword_table() -> Vec<(u64, u8)> {
    let mut v: Vec<(u64, u8)> = FUNC_WORDS.iter().map(|(w, c)| (word_hash(w), *c)).collect();
    v.sort_unstable();
    v
}

/// Class of a word given its running hash: 0 content, 1..=4 closed classes.
#[cfg(feature = "word-class")]
#[inline]
pub fn word_class(h: u64, table: &[(u64, u8)]) -> u8 {
    match table.binary_search_by_key(&h, |&(hh, _)| hh) {
        Ok(i) => table[i].1,
        Err(_) => 0,
    }
}

/// Which context an expert consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtxKind {
    /// A byte context of the given order.
    Order(usize),
    /// The current (case-folded) word prefix.
    Word,
    /// The previous completed word together with the current prefix.
    WordBigram,
    /// A17: the byte at the same column of the previous line, plus the byte to
    /// the left and the column bucket. Detects tabular/structured line geometry.
    Column,
    /// A17 negative control: the byte at a deliberately wrong column.
    ColumnShuffled,
    /// A17 null control: no previous-line byte at all (left byte + column only).
    /// Isolates the value of *vertical alignment* from that of the column bucket.
    ColumnNoLine,
    /// Phase 4.4: context conditioned on the match model's predicted byte and its
    /// match state (the LZMA matched-literal path, ledger X2/X3).
    MatchByte,
    /// Phase 4.4 control: the same expert with the predicted byte removed, so the
    /// archive effect of merely adding one more mixer input is separable.
    MatchByteConst,
    /// Phase 4.7: word-class context (prev word class + current word class).
    WordClass,
    /// Phase 4.7 control: the same expert keyed only on "inside a word".
    WordClassConst,
}

/// Closed-class English function words (ledger C8 word-type streams). The class
/// of the previous completed word and of the current prefix are cheap, bounded
/// context that the word/bigram experts do not expose directly.
#[cfg(feature = "word-class")]
pub const FUNC_WORDS: &[(&[u8], u8)] = &[
    (b"the", 1),
    (b"a", 1),
    (b"an", 1),
    (b"this", 1),
    (b"that", 1),
    (b"these", 1),
    (b"those", 1),
    (b"its", 1),
    (b"their", 1),
    (b"his", 1),
    (b"her", 1),
    (b"our", 1),
    (b"your", 1),
    (b"my", 1),
    (b"some", 1),
    (b"any", 1),
    (b"no", 1),
    (b"every", 1),
    (b"each", 1),
    (b"all", 1),
    (b"both", 1),
    (b"such", 1),
    (b"and", 2),
    (b"or", 2),
    (b"but", 2),
    (b"nor", 2),
    (b"of", 2),
    (b"to", 2),
    (b"in", 2),
    (b"on", 2),
    (b"at", 2),
    (b"for", 2),
    (b"with", 2),
    (b"by", 2),
    (b"from", 2),
    (b"as", 2),
    (b"into", 2),
    (b"over", 2),
    (b"under", 2),
    (b"about", 2),
    (b"after", 2),
    (b"before", 2),
    (b"between", 2),
    (b"during", 2),
    (b"through", 2),
    (b"against", 2),
    (b"among", 2),
    (b"up", 2),
    (b"down", 2),
    (b"out", 2),
    (b"off", 2),
    (b"than", 2),
    (b"if", 2),
    (b"because", 2),
    (b"while", 2),
    (b"when", 2),
    (b"where", 2),
    (b"i", 3),
    (b"he", 3),
    (b"she", 3),
    (b"it", 3),
    (b"we", 3),
    (b"they", 3),
    (b"you", 3),
    (b"me", 3),
    (b"him", 3),
    (b"us", 3),
    (b"them", 3),
    (b"who", 3),
    (b"which", 3),
    (b"whom", 3),
    (b"whose", 3),
    (b"is", 4),
    (b"are", 4),
    (b"was", 4),
    (b"were", 4),
    (b"be", 4),
    (b"been", 4),
    (b"being", 4),
    (b"am", 4),
    (b"has", 4),
    (b"have", 4),
    (b"had", 4),
    (b"do", 4),
    (b"does", 4),
    (b"did", 4),
    (b"will", 4),
    (b"would", 4),
    (b"shall", 4),
    (b"should", 4),
    (b"can", 4),
    (b"could", 4),
    (b"may", 4),
    (b"might", 4),
    (b"must", 4),
];

/// Specification of a single expert. Ablation removes a spec.
#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    pub kind: CtxKind,
    /// Log2 table size.
    pub bits: u32,
    /// Adaptation shift: `p += (target - p) >> rate`.
    pub rate: u32,
}

/// Information-inheritance mode for first-occupancy table slots (A3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoMode {
    /// Neutral cold start (`p = 0.5`) for every new slot.
    None,
    /// A newly occupied child slot inherits the parent expert's probability.
    Inherit,
    /// Negative control: inherit from a deliberately unrelated expert.
    Unrelated,
}

/// A direct context model: one adaptive probability per (context, partial byte)
/// index.
///
/// Table entries are initialised to `0`, which is a *sentinel* meaning
/// "never observed". Real probabilities are clamped to `[1, 65535]`, so `0` can
/// never be a genuine value and no separate occupancy array is needed.
#[derive(Debug, Clone)]
pub struct ContextModel {
    table: Vec<u16>,
    mask: usize,
    ctx: u32,
    idx: usize,
    rate: u32,
    /// The 16-bit probability most recently used for the current bit.
    pub last_p: u16,
}

impl ContextModel {
    pub fn new(bits: u32, rate: u32) -> Self {
        let n = 1usize << bits;
        ContextModel {
            table: vec![0u16; n],
            mask: n - 1,
            ctx: 0,
            idx: 0,
            rate,
            last_p: 32768,
        }
    }

    #[inline]
    pub fn set_context(&mut self, ctx: u32) {
        self.ctx = ctx;
    }

    /// Predict `P(bit = 1)`, stretched. `seed` is the probability used when this
    /// slot has never been occupied (information inheritance).
    #[inline]
    pub fn predict(&mut self, c0: u32, st: &StretchTable, seed: u16) -> i32 {
        self.idx = (self.ctx ^ c0.wrapping_mul(MIX_C)) as usize & self.mask;
        let mut t = self.table[self.idx];
        if t == 0 {
            t = seed.max(1);
            self.table[self.idx] = t;
        }
        self.last_p = t;
        st.stretch((t >> 4) as i32)
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        let target: i32 = if bit != 0 { 65535 } else { 0 };
        let p = self.table[self.idx] as i32;
        let np = p + ((target - p) >> self.rate);
        // Never emit the sentinel: real probabilities live in [1, 65535].
        self.table[self.idx] = np.clamp(1, 65535) as u16;
    }

    pub fn memory_bytes(&self) -> u64 {
        self.table.len() as u64 * 2
    }
}

/// Minimum match length required to start a match. Short matches are noise.
pub const MATCH_MIN: usize = 6;

/// A bitwise match model: if the recent context occurred before, predict that
/// the same continuation recurs.
#[derive(Debug, Clone)]
pub struct MatchModel {
    min_len: usize,
    /// Bytes excluded from the match context (Phase 4.2).
    gap: usize,
    /// Phase 4.5: scale confidence down with match distance.
    dist_scaled: bool,
    /// Distance of the current match (set when a match starts).
    off: usize,
    table: Vec<u32>,
    table_mask: usize,
    ptr: usize,
    len: u32,
    max_len: u32,
    pred_byte: u8,
    st_tab: Vec<i32>,
}

impl MatchModel {
    pub fn new(bits: u32, min_len: usize, gap: usize, dist_scaled: bool) -> Self {
        let n = 1usize << bits;
        let table = StretchTable::new();
        let mut st_tab = vec![0i32; 64];
        for (i, s) in st_tab.iter_mut().enumerate() {
            let p = crate::mixer::squash((i as i32 * 96).min(2047));
            *s = table.stretch(p.clamp(1, 4094));
        }
        MatchModel {
            min_len,
            gap,
            dist_scaled,
            off: 0,
            table: vec![0u32; n],
            table_mask: n - 1,
            ptr: 0,
            len: 0,
            max_len: 63,
            pred_byte: 0,
            st_tab,
        }
    }

    /// Returns the distance of a match that *started* on this boundary, if any
    /// (Phase 4.3 uses this to maintain the repeat-offset ring).
    pub fn byte_boundary(&mut self, buf: &[u8]) -> Option<usize> {
        let pos = buf.len();
        // The context is `min_len` bytes ending `gap` bytes before `pos`.
        if pos < self.min_len + self.gap {
            return None;
        }
        if self.len > 0 && self.ptr < pos && buf[self.ptr] == buf[pos - 1] {
            self.ptr += 1;
            self.len = (self.len + 1).min(self.max_len);
        } else {
            self.len = 0;
        }
        let cs = pos - self.min_len - self.gap;
        let ce = pos - self.gap;
        let ctx = hash_bytes(&buf[cs..ce]);
        let slot = (ctx as usize) & self.table_mask;
        let cand = self.table[slot] as usize;
        self.table[slot] = pos as u32;
        if self.len == 0 && cand >= self.min_len + self.gap && cand < pos {
            let hs = cand - self.min_len - self.gap;
            let he = cand - self.gap;
            if buf[hs..he] == buf[cs..ce] {
                self.ptr = cand;
                self.len = self.min_len as u32;
                self.off = pos - cand;
                return Some(pos - cand);
            }
        }
        None
    }

    #[inline]
    pub fn begin_byte(&mut self, buf: &[u8]) {
        if self.len > 0 && self.ptr < buf.len() {
            self.pred_byte = buf[self.ptr];
        } else {
            self.len = 0;
        }
    }

    #[inline]
    pub fn predict(&mut self, bitpos: u32) -> i32 {
        if self.len == 0 {
            return 0;
        }
        let bit = (self.pred_byte >> (7 - bitpos)) & 1;
        let eff = self.effective_len();
        let conf = self.st_tab[(eff as usize).min(self.st_tab.len() - 1)];
        if bit != 0 {
            conf
        } else {
            -conf
        }
    }

    /// Phase 4.5: distance-conditioned effective match length. Far matches must be
    /// longer to earn the same confidence (ledger A9/X5).
    #[inline]
    fn effective_len(&self) -> u32 {
        if !self.dist_scaled {
            return self.len;
        }
        let lg = (usize::BITS - self.off.max(1).leading_zeros()) as u32;
        let pen = lg.saturating_sub(10);
        self.len.saturating_sub(pen)
    }

    #[inline]
    pub fn update(&mut self, _bit: u32) {}

    /// The byte this tier currently predicts (Phase 4.4 matched-literal).
    #[inline]
    pub fn pred_byte(&self) -> u8 {
        self.pred_byte
    }

    #[inline]
    pub fn state(&self) -> usize {
        let l = self.effective_len();
        if self.len == 0 {
            0
        } else if l < 12 {
            1
        } else if l < 24 {
            2
        } else {
            3
        }
    }

    pub fn memory_bytes(&self) -> u64 {
        self.table.len() as u64 * 4 + (self.st_tab.len() * 4) as u64
    }
}

/// The ordered list of match tiers for a configuration: the short tier always,
/// then the optional long-distance tier (Phase 4.1).
fn match_tiers(cfg: &ModelConfig) -> Vec<MatchSpec> {
    cfg.matches.clone()
}

/// Compute the A3 inheritance parent of every expert: the most specific
/// lower-order expert already present. Word/bigram experts inherit from the
/// order-1 expert (or order-0) because their contexts have no order relation.
fn compute_parents(specs: &[ModelSpec]) -> Vec<Option<usize>> {
    let mut parents = vec![None; specs.len()];
    let mut last_order: Option<usize> = None;
    let mut order1: Option<usize> = None;
    let mut order0: Option<usize> = None;
    for (i, s) in specs.iter().enumerate() {
        match s.kind {
            CtxKind::Order(0) => {
                order0 = Some(i);
                last_order = Some(i);
                parents[i] = None;
            }
            CtxKind::Order(1) => {
                order1 = Some(i);
                parents[i] = last_order;
                last_order = Some(i);
            }
            CtxKind::Order(_) => {
                parents[i] = last_order;
                last_order = Some(i);
            }
            CtxKind::Word | CtxKind::WordBigram => {
                parents[i] = order1.or(order0);
            }
            CtxKind::Column | CtxKind::ColumnShuffled | CtxKind::ColumnNoLine => {
                parents[i] = order1.or(order0);
            }
            CtxKind::MatchByte | CtxKind::MatchByteConst => {
                parents[i] = order1.or(order0);
            }
            CtxKind::WordClass | CtxKind::WordClassConst => {
                parents[i] = order1.or(order0);
            }
        }
    }
    parents
}

/// A match tier: table size, minimum match length, and a context gap.
/// `gap = 0` is the dense tier (context ends at the current byte); `gap = k > 0`
/// excludes the `k` most recent bytes from the context, so the model can catch
/// repeats whose immediately preceding bytes differ (Phase 4.2, ledger C9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchSpec {
    pub bits: u32,
    pub min_len: usize,
    pub gap: usize,
}

/// Configuration for the classical prediction floor.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub specs: Vec<ModelSpec>,
    /// Ordered match tiers (Phase 4.1/4.2). The first is the dense short tier.
    pub matches: Vec<MatchSpec>,
    /// Phase 4.3: number of repeat-offset predictors (0 = off).
    pub rep_offsets: usize,
    /// Phase 4.5: distance-scale match confidence.
    pub dist_match: bool,
    pub mixer_lr: i32,
    pub apm1_ctx: usize,
    pub apm2_ctx: usize,
    /// A3 information-inheritance mode for first-occupancy slots.
    pub info: InfoMode,
}

impl ModelConfig {
    /// The full default floor for a corpus of `n` bytes.
    pub fn for_size(n: u64) -> Self {
        let bits = if n <= 2_000_000 {
            18
        } else if n <= 20_000_000 {
            20
        } else if n <= 200_000_000 {
            22
        } else {
            24
        };
        let match_bits = bits;
        let word_bits = bits.min(22);
        // The expert ladder. Higher orders use the same table size and rely on
        // hashing; collisions are charged as lost probability, not as bytes.
        let specs = vec![
            ModelSpec {
                kind: CtxKind::Order(0),
                bits: bits.min(20),
                rate: 4,
            },
            ModelSpec {
                kind: CtxKind::Order(1),
                bits,
                rate: 4,
            },
            ModelSpec {
                kind: CtxKind::Order(2),
                bits,
                rate: 4,
            },
            ModelSpec {
                kind: CtxKind::Order(3),
                bits,
                rate: 5,
            },
            ModelSpec {
                kind: CtxKind::Order(4),
                bits,
                rate: 5,
            },
            ModelSpec {
                kind: CtxKind::Order(5),
                bits,
                rate: 5,
            },
            ModelSpec {
                kind: CtxKind::Order(6),
                bits,
                rate: 5,
            },
            ModelSpec {
                kind: CtxKind::Order(8),
                bits,
                rate: 6,
            },
            ModelSpec {
                kind: CtxKind::Order(12),
                bits,
                rate: 6,
            },
            ModelSpec {
                kind: CtxKind::Order(16),
                bits,
                rate: 6,
            },
            ModelSpec {
                kind: CtxKind::Word,
                bits: word_bits,
                rate: 5,
            },
            ModelSpec {
                kind: CtxKind::WordBigram,
                bits: word_bits,
                rate: 5,
            },
        ];
        ModelConfig {
            specs,
            matches: vec![MatchSpec {
                bits: match_bits,
                min_len: MATCH_MIN,
                gap: 0,
            }],
            rep_offsets: 0,
            dist_match: false,
            mixer_lr: 12,
            apm1_ctx: 4096,
            apm2_ctx: 65536,
            info: InfoMode::None,
        }
    }

    /// Phase 4.7: append a word-class context expert. `const_ctl` drops the
    /// closed-class information and keeps only "inside a word".
    pub fn with_word_class(mut self, const_ctl: bool) -> Self {
        let kind = if const_ctl {
            CtxKind::WordClassConst
        } else {
            CtxKind::WordClass
        };
        let bits = self.specs[0].bits.min(20);
        self.specs.push(ModelSpec {
            kind,
            bits,
            rate: 5,
        });
        self
    }

    /// Phase 4.5: enable distance-conditioned match confidence.
    pub fn with_dist_match(mut self) -> Self {
        self.dist_match = true;
        self
    }

    /// Phase 4.3: enable `n` repeat-offset predictors fed by the recent match
    /// distance ring.
    pub fn with_rep_offsets(mut self, n: usize) -> Self {
        self.rep_offsets = n;
        self
    }

    /// Phase 4.1: append a long-distance match tier with the given minimum match
    /// length. The short tier still exists; this adds a separate index that is
    /// not evicted by short-range traffic.
    pub fn with_match2(mut self, min_len: usize) -> Self {
        let bits = self.matches[0].bits;
        self.matches.push(MatchSpec {
            bits,
            min_len,
            gap: 0,
        });
        self
    }

    /// Phase 4.2: append a sparse (gapped) match tier.
    pub fn with_match_tier(mut self, min_len: usize, gap: usize) -> Self {
        let bits = self.matches[0].bits;
        self.matches.push(MatchSpec { bits, min_len, gap });
        self
    }

    /// Set the information-inheritance mode (A3).
    pub fn with_info(mut self, info: InfoMode) -> Self {
        self.info = info;
        self
    }

    /// A17: append a previous-line/column expert. `shuffled` selects the
    /// negative control (a deliberately wrong column).
    pub fn with_column(self, shuffled: bool) -> Self {
        let kind = if shuffled {
            CtxKind::ColumnShuffled
        } else {
            CtxKind::Column
        };
        self.with_column_kind(kind)
    }

    /// A17: append an expert of the given column family.
    pub fn with_column_kind(mut self, kind: CtxKind) -> Self {
        let bits = self.specs[0].bits.min(20);
        self.specs.push(ModelSpec {
            kind,
            bits,
            rate: 5,
        });
        self
    }

    /// Phase 4.4: append a matched-literal expert. `const_ctl` selects the
    /// control that removes the predicted byte.
    pub fn with_match_byte(mut self, const_ctl: bool) -> Self {
        let kind = if const_ctl {
            CtxKind::MatchByteConst
        } else {
            CtxKind::MatchByte
        };
        let bits = self.specs[0].bits.min(20);
        self.specs.push(ModelSpec {
            kind,
            bits,
            rate: 5,
        });
        self
    }

    /// A20: map a tuning variant to a mixer learning rate. Variant 0 is the
    /// frozen parent behaviour (lr = 12); higher variants explore the update-law
    /// family without changing the executable, since the variant is carried in
    /// the archive header and applied identically by the decoder.
    pub fn with_tune(mut self, tune: u8) -> Self {
        const LRS: [i32; 16] = [
            12, 4, 6, 8, 10, 16, 20, 24, 32, 40, 48, 64, 96, 128, 192, 256,
        ];
        self.mixer_lr = LRS[(tune & 15) as usize];
        self
    }

    /// A configuration with a chosen subset of experts, for ablation.
    pub fn ablated(&self, keep: &[usize]) -> ModelConfig {
        let mut c = self.clone();
        c.specs = keep.iter().map(|&i| self.specs[i]).collect();
        c
    }

    pub fn memory_bytes(&self) -> u64 {
        let mut m = 0u64;
        for s in &self.specs {
            m += (1u64 << s.bits) * 2;
        }
        for t in &self.matches {
            m += (1u64 << t.bits) * 4;
        }
        m += (self.apm1_ctx as u64) * 33 * 4;
        m += (self.apm2_ctx as u64) * 33 * 4;
        m
    }
}

/// The composite predictor: experts -> mixer -> calibration.
pub struct Predictor {
    models: Vec<ContextModel>,
    specs: Vec<ModelSpec>,
    ctx: Vec<u32>,
    /// For each expert, the lower-order expert that seeds its cold slots (A3).
    parent: Vec<Option<usize>>,
    info: InfoMode,
    match_models: Vec<MatchModel>,
    /// Phase 4.7: sorted closed-class word table.
    #[cfg_attr(not(feature = "word-class"), allow(dead_code))]
    fnwords: Vec<(u64, u8)>,
    /// Phase 4.3: MRU ring of recent match distances and their confidences.
    rep: Vec<usize>,
    rep_conf: Vec<i32>,
    mixer: Mixer,
    apm1: Apm,
    apm2: Apm,
    st: StretchTable,
    buf: Vec<u8>,
    c0: u32,
    bitpos: u32,
    last_byte: u8,
    word_cur: u64,
    word_prev: u64,
    /// Start offset of the current line (after the most recent newline).
    line_start: usize,
    /// Start offset of the previous line.
    prev_line_start: usize,
    pr: i32,
    inputs: Vec<i32>,
}

impl Predictor {
    pub fn new(cfg: &ModelConfig, buf_capacity: usize) -> Self {
        let models: Vec<ContextModel> = cfg
            .specs
            .iter()
            .map(|s| ContextModel::new(s.bits, s.rate))
            .collect();
        let n_inputs = models.len() + match_tiers(cfg).len() + cfg.rep_offsets;
        let mut match_models: Vec<MatchModel> = Vec::new();
        for spec in match_tiers(cfg) {
            match_models.push(MatchModel::new(
                spec.bits,
                spec.min_len,
                spec.gap,
                cfg.dist_match,
            ));
        }
        Predictor {
            models,
            parent: compute_parents(&cfg.specs),
            info: cfg.info,
            specs: cfg.specs.clone(),
            ctx: vec![0; cfg.specs.len()],
            match_models,
            fnwords: {
                #[cfg(feature = "word-class")]
                {
                    fnword_table()
                }
                #[cfg(not(feature = "word-class"))]
                {
                    Vec::new()
                }
            },
            rep: vec![0; cfg.rep_offsets],
            rep_conf: vec![0; cfg.rep_offsets],
            mixer: Mixer::new(n_inputs, 4096),
            apm1: Apm::new(cfg.apm1_ctx, 7),
            apm2: Apm::new(cfg.apm2_ctx, 7),
            st: StretchTable::new(),
            buf: Vec::with_capacity(buf_capacity),
            c0: 1,
            bitpos: 0,
            last_byte: 0,
            word_cur: 0,
            word_prev: 0,
            line_start: 0,
            prev_line_start: 0,
            pr: 2048,
            inputs: vec![0; n_inputs],
        }
    }

    fn set_learning_rate(&mut self, lr: i32) {
        self.mixer.set_learning_rate(lr);
    }

    /// Recompute every expert context for the next byte from the buffer tail and
    /// the word state.
    fn refresh_contexts(&mut self) {
        let n = self.buf.len();
        // Phase 4.4: the best-tier match prediction, for the matched-literal expert.
        let mut mb_pb = 0u8;
        let mut mb_state = 0u8;
        for m in &self.match_models {
            let s = m.state() as u8;
            if s >= mb_state {
                mb_state = s;
                mb_pb = m.pred_byte();
            }
        }
        // `mb_pb` is only consumed by the matched-literal expert (feature-gated).
        let _ = (mb_pb, mb_state);
        for (i, spec) in self.specs.iter().enumerate() {
            let c = match spec.kind {
                CtxKind::Order(o) => {
                    let o = o.min(n);
                    hash_bytes(&self.buf[n - o..n])
                }
                CtxKind::Word => {
                    // Distinguish "inside a word" from "between words".
                    let h = word_add(self.word_cur, 0x5f);
                    h as u32
                }
                CtxKind::WordBigram => {
                    let h = word_add(self.word_prev ^ 0x9E37_79B9_7F4A_7C15, 0x20);
                    let h = word_add(h, (self.word_cur & 0xff) as u8) ^ (self.word_cur >> 8);
                    h as u32
                }
                CtxKind::Column | CtxKind::ColumnShuffled | CtxKind::ColumnNoLine => {
                    #[cfg(feature = "column-model")]
                    {
                        let pos = n;
                        let col = pos.saturating_sub(self.line_start);
                        let prev_len = if self.line_start > 0 {
                            self.line_start.saturating_sub(1 + self.prev_line_start)
                        } else {
                            0
                        };
                        // `above` is the byte at the aligned column of the
                        // previous line; controls deliberately corrupt it.
                        let above = match spec.kind {
                            CtxKind::ColumnNoLine => 0,
                            CtxKind::ColumnShuffled => {
                                if prev_len > 0 {
                                    let c = (col + 1) % prev_len;
                                    self.buf[self.prev_line_start + c]
                                } else {
                                    0
                                }
                            }
                            _ => {
                                if col < prev_len {
                                    self.buf[self.prev_line_start + col]
                                } else {
                                    0
                                }
                            }
                        };
                        let left = if col > 0 { self.buf[pos - 1] } else { 0 };
                        hash_bytes(&[above, left, (col.min(63)) as u8])
                    }
                    #[cfg(not(feature = "column-model"))]
                    {
                        // Measurement build without the mechanism: neutral.
                        0
                    }
                }
                CtxKind::MatchByte | CtxKind::MatchByteConst => {
                    #[cfg(feature = "match-byte")]
                    {
                        let pb = if matches!(spec.kind, CtxKind::MatchByte) {
                            mb_pb
                        } else {
                            0
                        };
                        hash_bytes(&[pb, mb_state])
                    }
                    #[cfg(not(feature = "match-byte"))]
                    {
                        0
                    }
                }
                CtxKind::WordClass | CtxKind::WordClassConst => {
                    #[cfg(feature = "word-class")]
                    {
                        let wo = if self.word_cur != 0 { 1u8 } else { 0u8 };
                        if matches!(spec.kind, CtxKind::WordClass) {
                            let pc = word_class(self.word_prev, &self.fnwords);
                            let cc = word_class(self.word_cur, &self.fnwords);
                            hash_bytes(&[pc, cc, wo])
                        } else {
                            hash_bytes(&[wo])
                        }
                    }
                    #[cfg(not(feature = "word-class"))]
                    {
                        0
                    }
                }
            };
            self.ctx[i] = c;
        }
        for (i, c) in self.ctx.iter().enumerate() {
            self.models[i].set_context(*c);
        }
    }

    /// Called after each byte is appended: update word state and match model.
    fn obs_core(&mut self) {
        if let Some(&b) = self.buf.last() {
            let lc = b.to_ascii_lowercase();
            if lc.is_ascii_alphabetic() || lc == b'\'' {
                self.word_cur = word_add(self.word_cur, lc);
            } else {
                if self.word_cur != 0 {
                    self.word_prev = self.word_cur;
                }
                self.word_cur = 0;
            }
            self.last_byte = b;
            if b == b'\n' {
                self.prev_line_start = self.line_start;
                self.line_start = self.buf.len();
            }
        }
        // Match tiers first, so their predicted bytes are current for the
        // matched-literal expert read by refresh_contexts (Phase 4.4).
        for mm in &mut self.match_models {
            if let Some(d) = mm.byte_boundary(&self.buf) {
                if !self.rep.is_empty() && d > 0 && self.rep[0] != d {
                    self.rep.rotate_right(1);
                    self.rep[0] = d;
                }
            }
            mm.begin_byte(&self.buf);
        }
        // Phase 4.3: score the repeat-offset predictions that were made for the
        // byte just appended.
        if !self.rep.is_empty() {
            let q = self.buf.len();
            let idx = q - 1;
            let actual = self.buf[idx];
            for k in 0..self.rep.len() {
                let d = self.rep[k];
                if d > 0 && idx >= d {
                    if self.buf[idx - d] == actual {
                        self.rep_conf[k] += (2047 - self.rep_conf[k]) >> 4;
                    } else {
                        self.rep_conf[k] -= self.rep_conf[k] >> 3;
                    }
                } else {
                    self.rep_conf[k] -= self.rep_conf[k] >> 3;
                }
            }
        }
        self.refresh_contexts();
    }

    /// Initialise contexts (call once, before coding, with an empty buffer).
    pub fn prime(&mut self) {
        self.refresh_contexts();
        for mm in &mut self.match_models {
            mm.begin_byte(&self.buf);
        }
    }

    /// `P(bit = 1)` in `[1, 4094]`.
    #[inline]
    pub fn predict(&mut self) -> u32 {
        let nm = self.models.len();
        for i in 0..nm {
            // A3: seed a cold slot from the parent expert's probability for this
            // same bit. Both encoder and decoder apply the identical rule, so no
            // side information is needed.
            let seed: u16 = match self.info {
                InfoMode::None => 32768,
                InfoMode::Inherit => self.parent[i]
                    .map(|j| self.models[j].last_p)
                    .unwrap_or(32768),
                InfoMode::Unrelated => {
                    let base = self.parent[i].unwrap_or(i);
                    let alt = (base + 1 + nm / 2) % nm;
                    self.models[alt].last_p
                }
            };
            self.inputs[i] = self.models[i].predict(self.c0, &self.st, seed);
        }
        let mstate = self
            .match_models
            .iter()
            .map(|m| m.state())
            .max()
            .unwrap_or(0);
        for (k, mm) in self.match_models.iter_mut().enumerate() {
            self.inputs[nm + k] = mm.predict(self.bitpos);
        }
        // Phase 4.3: repeat-offset predictors.
        let rep_base = nm + self.match_models.len();
        let q = self.buf.len();
        for k in 0..self.rep.len() {
            let d = self.rep[k];
            self.inputs[rep_base + k] = if d > 0 && q >= d {
                let pb = self.buf[q - d];
                let bit = (pb >> (7 - self.bitpos)) & 1;
                let c = self.rep_conf[k].clamp(0, 2047);
                if bit != 0 {
                    c
                } else {
                    -c
                }
            } else {
                0
            };
        }

        let word_open = if self.word_cur != 0 { 1usize } else { 0 };
        let rep_active = if self.rep.iter().any(|&d| d > 0) {
            1usize
        } else {
            0usize
        };
        let mix_cx =
            (self.c0 as usize & 0xff) | (mstate << 8) | (word_open << 10) | (rep_active << 11);
        let raw = self.mixer.mix(&self.inputs, mix_cx);

        let a1 = self.apm1.predict(raw, (self.c0 as usize) | (mstate << 8));
        let a2 = self.apm2.predict(raw, self.last_byte as usize);
        let pr = (raw + a1 + 2 * a2 + 2) >> 2;
        self.pr = pr.clamp(1, 4094);
        self.pr as u32
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        self.mixer.update(bit);
        self.apm1.update(bit);
        self.apm2.update(bit);
        for m in &mut self.models {
            m.update(bit);
        }
        for mm in &mut self.match_models {
            mm.update(bit);
        }

        self.c0 = (self.c0 << 1) | bit;
        self.bitpos += 1;
        if self.bitpos == 8 {
            let byte = (self.c0 & 0xff) as u8;
            self.buf.push(byte);
            self.c0 = 1;
            self.bitpos = 0;
            self.obs_core();
        }
    }

    pub fn output(&self) -> &[u8] {
        &self.buf
    }

    pub fn memory_bytes(&self) -> u64 {
        let mut m: u64 = 0;
        for x in &self.models {
            m += x.memory_bytes();
        }
        for mm in &self.match_models {
            m += mm.memory_bytes();
        }
        m + self.buf.capacity() as u64
    }
}

/// A self-contained byte-level context-mixing model.
pub struct Cm {
    predictor: Predictor,
}

impl Cm {
    pub fn new(cfg: &ModelConfig, buf_capacity: usize) -> Self {
        let mut predictor = Predictor::new(cfg, buf_capacity);
        predictor.set_learning_rate(cfg.mixer_lr);
        predictor.prime();
        Cm { predictor }
    }

    #[inline]
    pub fn predict(&mut self) -> u32 {
        self.predictor.predict()
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        self.predictor.update(bit);
    }

    #[inline]
    pub fn output(&self) -> &[u8] {
        self.predictor.output()
    }

    pub fn memory_bytes(&self) -> u64 {
        self.predictor.memory_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(hash_bytes(b"abc"), hash_bytes(b"abc"));
        assert_ne!(hash_bytes(b"abc"), hash_bytes(b"abd"));
    }

    #[test]
    fn context_model_learns_a_bit() {
        let st = StretchTable::new();
        let mut m = ContextModel::new(16, 4);
        m.set_context(42);
        for _ in 0..2000 {
            m.predict(1, &st, 32768);
            m.update(1);
        }
        let p = m.predict(1, &st, 32768);
        assert!(p > 1500, "p={p}");
    }

    #[test]
    fn info_inheritance_seeds_cold_slots() {
        let st = StretchTable::new();
        let mut m = ContextModel::new(12, 4);
        m.set_context(7);
        // Cold slot seeded with a confident parent must start confident.
        let p = m.predict(1, &st, 60000);
        assert!(p > 400, "cold slot ignored its seed: p={p}");
        // A neutral seed must start at the neutral probability.
        let mut m2 = ContextModel::new(12, 4);
        m2.set_context(7);
        let pn = m2.predict(1, &st, 32768);
        assert!(
            pn.abs() < 200,
            "neutral seed should be near 0 log-odds: p={pn}"
        );
        assert!(p > pn);
    }

    #[test]
    fn parents_form_a_lower_order_chain() {
        let c = ModelConfig::for_size(1_000_000);
        let parents = compute_parents(&c.specs);
        // First spec (order 0) has no parent; every order-k>0 has one.
        assert_eq!(parents[0], None);
        assert!(parents[1..].iter().all(|p| p.is_some()));
    }

    #[test]
    fn match_model_finds_a_repeat() {
        let buf: Vec<u8> = b"hello world hello world".to_vec();
        let mut mm = MatchModel::new(16, MATCH_MIN, 0, false);
        for i in 1..=buf.len() {
            mm.byte_boundary(&buf[..i]);
        }
        mm.begin_byte(&buf);
        assert!(mm.len > 0, "match not found");
    }

    #[test]
    fn config_ablation_keeps_subset() {
        let c = ModelConfig::for_size(1_000_000);
        assert!(c.specs.len() >= 4);
        let a = c.ablated(&[0, 1]);
        assert_eq!(a.specs.len(), 2);
        assert_eq!(a.specs[0].kind, CtxKind::Order(0));
        assert_eq!(a.specs[1].kind, CtxKind::Order(1));
        assert_eq!(c.specs[c.specs.len() - 1].kind, CtxKind::WordBigram);
    }
}
