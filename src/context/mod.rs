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

/// Which context an expert consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtxKind {
    /// A byte context of the given order.
    Order(usize),
    /// The current (case-folded) word prefix.
    Word,
    /// The previous completed word together with the current prefix.
    WordBigram,
}

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
    table: Vec<u32>,
    table_mask: usize,
    ptr: usize,
    len: u32,
    max_len: u32,
    pred_byte: u8,
    st_tab: Vec<i32>,
}

impl MatchModel {
    pub fn new(bits: u32) -> Self {
        let n = 1usize << bits;
        let table = StretchTable::new();
        let mut st_tab = vec![0i32; 64];
        for (i, s) in st_tab.iter_mut().enumerate() {
            let p = crate::mixer::squash((i as i32 * 96).min(2047));
            *s = table.stretch(p.clamp(1, 4094));
        }
        MatchModel {
            min_len: MATCH_MIN,
            table: vec![0u32; n],
            table_mask: n - 1,
            ptr: 0,
            len: 0,
            max_len: 63,
            pred_byte: 0,
            st_tab,
        }
    }

    pub fn byte_boundary(&mut self, buf: &[u8]) {
        let pos = buf.len();
        if pos < self.min_len {
            return;
        }
        if self.len > 0 && self.ptr < pos && buf[self.ptr] == buf[pos - 1] {
            self.ptr += 1;
            self.len = (self.len + 1).min(self.max_len);
        } else {
            self.len = 0;
        }
        let ctx = hash_bytes(&buf[pos - self.min_len..pos]);
        let slot = (ctx as usize) & self.table_mask;
        let cand = self.table[slot] as usize;
        self.table[slot] = pos as u32;
        if self.len == 0 && cand >= self.min_len && cand < pos {
            if buf[cand - self.min_len..cand] == buf[pos - self.min_len..pos] {
                self.ptr = cand;
                self.len = self.min_len as u32;
            }
        }
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
        let conf = self.st_tab[(self.len as usize).min(self.st_tab.len() - 1)];
        if bit != 0 {
            conf
        } else {
            -conf
        }
    }

    #[inline]
    pub fn update(&mut self, _bit: u32) {}

    #[inline]
    pub fn state(&self) -> usize {
        if self.len == 0 {
            0
        } else if self.len < 12 {
            1
        } else if self.len < 24 {
            2
        } else {
            3
        }
    }

    pub fn memory_bytes(&self) -> u64 {
        self.table.len() as u64 * 4 + (self.st_tab.len() * 4) as u64
    }
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
        }
    }
    parents
}

/// Configuration for the classical prediction floor.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub specs: Vec<ModelSpec>,
    pub match_bits: u32,
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
            match_bits,
            mixer_lr: 12,
            apm1_ctx: 4096,
            apm2_ctx: 65536,
            info: InfoMode::None,
        }
    }

    /// Set the information-inheritance mode (A3).
    pub fn with_info(mut self, info: InfoMode) -> Self {
        self.info = info;
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
        m += (1u64 << self.match_bits) * 4;
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
    match_model: MatchModel,
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
        let n_inputs = models.len() + 1;
        Predictor {
            models,
            parent: compute_parents(&cfg.specs),
            info: cfg.info,
            specs: cfg.specs.clone(),
            ctx: vec![0; cfg.specs.len()],
            match_model: MatchModel::new(cfg.match_bits),
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
        }
        self.refresh_contexts();
        self.match_model.byte_boundary(&self.buf);
        self.match_model.begin_byte(&self.buf);
    }

    /// Initialise contexts (call once, before coding, with an empty buffer).
    pub fn prime(&mut self) {
        self.refresh_contexts();
        self.match_model.begin_byte(&self.buf);
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
        let mstate = self.match_model.state();
        self.inputs[nm] = self.match_model.predict(self.bitpos);

        let word_open = if self.word_cur != 0 { 1usize } else { 0 };
        let mix_cx = (self.c0 as usize & 0xff) | (mstate << 8) | (word_open << 10);
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
        self.match_model.update(bit);

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
        m += self.match_model.memory_bytes();
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
        let mut mm = MatchModel::new(16);
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
