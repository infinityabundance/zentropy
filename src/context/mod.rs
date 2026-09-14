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

/// Phase 6.10: reduce a lowercase word to a conservative stem by stripping a
/// small, deterministic set of common English suffixes. Stems of length < 3 are
/// left untouched so the transform never destroys a short word. Purely
/// mechanical, so encoder and decoder agree bit-for-bit.
#[inline]
pub fn stem_bytes(w: &[u8]) -> &[u8] {
    let s = w;
    let n = s.len();
    if n > 5 && s.ends_with(b"ingly") {
        return &s[..n - 5];
    }
    if n > 4 && s.ends_with(b"ies") {
        return &s[..n - 3];
    }
    if n > 4 && s.ends_with(b"ing") {
        return &s[..n - 3];
    }
    if n > 4 && s.ends_with(b"ely") {
        return &s[..n - 3];
    }
    if n > 4 && s.ends_with(b"ly") {
        return &s[..n - 2];
    }
    if n > 3 && s.ends_with(b"ed") {
        return &s[..n - 2];
    }
    if n > 3 && s.ends_with(b"es") {
        return &s[..n - 2];
    }
    if n > 2 && s.ends_with(b"s") && !s.ends_with(b"ss") {
        return &s[..n - 1];
    }
    s
}

/// Fold a byte string (already lowercase) into a 64-bit word hash.
#[inline]
fn fold_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0;
    for &b in bytes {
        h = word_add(h, b);
    }
    h
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
    /// Phase 6.1: a byte context of the given order modelled by a bucketed bit
    /// history and the shared adaptive StateMap (the indirect representation).
    StateOrder(usize),
    /// Phase 6.5: a *sparse* (gapped) byte context. The context is the `order`
    /// bytes ending `gap` bytes before the current position, so the `gap` most
    /// recent bytes are excluded. `gap = 1` skips one byte (ledger P6).
    Sparse(usize, usize),
    /// Phase 6.2/6.6: an indirect context model. `order` selects the source byte
    /// context; the model keeps, per source slot, a rolling `hist`-byte history
    /// of what was last seen after that context, and the expert is keyed on the
    /// pair (source slot, learned state). `chained` selects the second-order
    /// form, where the state tracked is the history of the state itself.
    Indirect {
        order: usize,
        hist: usize,
        chained: bool,
    },
    /// Phase 6.2 negative control: the same machine reading the learned state
    /// from a deliberately wrong source slot.
    IndirectCtl { order: usize, hist: usize },
    /// Phase 6.10: the stem-folded current word (a word *model*, not a
    /// transform — Phase 4.6 showed the stem transform loses).
    Stem,
    /// Phase 6.10 width control: the raw current word, i.e. a duplicate of the
    /// existing word expert with the same mixer width.
    StemCtl,
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
    /// Phase 6.7: per-slot checksum, empty unless collision control is enabled.
    chk: Vec<u8>,
    mask: usize,
    ctx: u32,
    idx: usize,
    rate: u32,
    /// Phase 6.7: 0 = off, 1 = real checksum, 2 = constant (cost-only control).
    verify: u8,
    /// Number of low index bits; the checksum is taken from the bits just above.
    chk_shift: u32,
    /// The 16-bit probability most recently used for the current bit.
    pub last_p: u16,
}

impl ContextModel {
    pub fn new(bits: u32, rate: u32) -> Self {
        Self::new_verify(bits, rate, 0)
    }

    /// Phase 6.7: build with optional collision control. When `verify == 1` each
    /// slot carries an 8-bit checksum of the context that last wrote it, and a
    /// mismatch is treated as an unoccupied (cold) slot. `verify == 2` allocates
    /// the checksums but never rejects, isolating the logic from its memory cost.
    pub fn new_verify(bits: u32, rate: u32, verify: u8) -> Self {
        let n = 1usize << bits;
        let chk = if verify == 0 {
            Vec::new()
        } else {
            vec![0u8; n]
        };
        ContextModel {
            table: vec![0u16; n],
            chk,
            mask: n - 1,
            ctx: 0,
            idx: 0,
            rate,
            verify,
            chk_shift: bits,
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
        let h = (self.ctx ^ c0.wrapping_mul(MIX_C)) as usize;
        self.idx = h & self.mask;
        if self.verify != 0 {
            let want = if self.verify == 1 {
                (h >> self.chk_shift) as u8
            } else {
                0
            };
            if self.chk[self.idx] != want {
                self.chk[self.idx] = want;
                self.table[self.idx] = 0;
            }
        }
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
        self.table.len() as u64 * 2 + self.chk.len() as u64
    }
}

/// Phase 6.1: a context slot storing a bucketed bit history as `(n0 << 4) | n1`.
/// The counts saturate at 15, so the state is one byte.
#[derive(Debug, Clone)]
pub struct StateModel {
    table: Vec<u8>,
    mask: usize,
    ctx: u32,
    idx: usize,
    /// State selected by the last `begin`, used by the shared state map.
    pub cur: u8,
}

impl StateModel {
    pub fn new(bits: u32) -> Self {
        let n = 1usize << bits;
        StateModel {
            table: vec![0u8; n],
            mask: n - 1,
            ctx: 0,
            idx: 0,
            cur: 0,
        }
    }

    #[inline]
    pub fn set_context(&mut self, c: u32) {
        self.ctx = c;
    }

    #[inline]
    pub fn begin(&mut self, c0: u32) -> u8 {
        self.idx = (self.ctx ^ c0.wrapping_mul(MIX_C)) as usize & self.mask;
        self.cur = self.table[self.idx];
        self.cur
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        let s = self.cur as usize;
        let mut n0 = (s >> 4) as u32;
        let mut n1 = (s & 15) as u32;
        if bit != 0 {
            if n1 < 15 {
                n1 += 1;
            }
        } else if n0 < 15 {
            n0 += 1;
        }
        self.table[self.idx] = ((n0 << 4) | n1) as u8;
    }

    pub fn memory_bytes(&self) -> u64 {
        self.table.len() as u64
    }
}

/// The shared adaptive map from a bucketed bit-history state to a probability
/// (the PAQ `StateMap`). It is trained across every state context, so the model
/// learns how much to trust each `(n0, n1)` configuration.
#[derive(Debug, Clone)]
pub struct StateMap {
    t: Vec<u16>,
}

impl StateMap {
    pub fn new() -> Self {
        let mut t = vec![0u16; 256];
        for (s, v) in t.iter_mut().enumerate() {
            let n0 = (s >> 4) as u32;
            let n1 = (s & 15) as u32;
            *v = (((n1 + 1) * 65536) / (n0 + n1 + 2)).clamp(1, 65535) as u16;
        }
        StateMap { t }
    }

    #[inline]
    pub fn predict(&self, st: u8, st_tab: &StretchTable) -> i32 {
        st_tab.stretch((self.t[st as usize] >> 4) as i32)
    }

    #[inline]
    pub fn update(&mut self, st: u8, bit: u32) {
        let target: i32 = if bit != 0 { 65535 } else { 0 };
        let p = self.t[st as usize] as i32;
        self.t[st as usize] = (p + ((target - p) >> 6)).clamp(1, 65535) as u16;
    }

    pub fn memory_bytes(&self) -> u64 {
        self.t.len() as u64 * 2
    }
}

impl Default for StateMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Phase 6.2/6.6: the learned state of an indirect context model.
///
/// A direct order-N model learns `P(next bit | last N bytes)`. An indirect model
/// instead learns what *usually follows* a source context and feeds that learned
/// state back in as the context. Per source slot we keep a rolling history of the
/// bytes last observed after that context; the expert is keyed on the pair
/// `(source slot, history)`, so it can react to non-stationarity that a static
/// context table smooths away.
#[derive(Debug, Clone)]
pub struct IndirectState {
    /// 64-bit slots: low 32 bits = rolling byte history, high 32 bits = the
    /// previous history (only read in the chained form).
    table: Vec<u64>,
    mask: usize,
    /// Bytes of history retained per slot (1..=4).
    hist: usize,
    /// Second-order form: the expert context also sees the previous state.
    chained: bool,
    /// Slot key used at the previous byte boundary.
    last_key: usize,
    valid: bool,
    /// Negative control: associate state with a permuted source slot.
    wrong: bool,
}

impl IndirectState {
    /// `bits` bounds the source-slot table (capped at 18 bits: 2 MiB of u64).
    pub fn new(bits: u32, hist: usize, chained: bool, wrong: bool) -> Self {
        let n = 1usize << bits.min(18);
        IndirectState {
            table: vec![0u64; n],
            mask: n - 1,
            hist: hist.clamp(1, 4),
            chained,
            last_key: 0,
            valid: false,
            wrong,
        }
    }

    #[inline]
    fn key(&self, src: u32) -> usize {
        let slot = (src as usize) & self.mask;
        if self.wrong {
            slot ^ (self.mask >> 1)
        } else {
            slot
        }
    }

    /// Fold the byte just appended into the slot that predicted it, then return
    /// the expert context for the next byte.
    #[inline]
    pub fn refresh(&mut self, buf: &[u8], src: u32) -> u32 {
        if self.valid {
            let b = buf.last().copied().unwrap_or(0) as u32;
            let m = if self.hist >= 4 {
                u32::MAX
            } else {
                (1u32 << (8 * self.hist)) - 1
            };
            let v = self.table[self.last_key];
            let cur = v as u32;
            let next = ((cur << 8) | b) & m;
            self.table[self.last_key] = (next as u64) | ((cur as u64) << 32);
        }
        let k = self.key(src);
        self.last_key = k;
        self.valid = true;
        let v = self.table[k];
        let cur = v as u32;
        if self.chained {
            let prev = (v >> 32) as u32;
            cur ^ prev.wrapping_mul(MIX_C) ^ (k as u32).wrapping_mul(HASH_C as u32)
        } else {
            cur ^ (k as u32).wrapping_mul(MIX_C)
        }
    }

    pub fn memory_bytes(&self) -> u64 {
        self.table.len() as u64 * 8
    }
}

/// Scale used by the PPM symbol-mass arithmetic.
const PPM_SCALE: i64 = 1 << 16;

/// One order of the Phase 6.8 PPM-C model.
///
/// Each hashed context owns a 256-leaf binary count tree (`tree[1]` is the
/// context total; symbol `s` is leaf `256 + s`), so the mass of any byte prefix
/// is an O(1) lookup instead of a 256-symbol scan. `seen` is a 256-bit bitmap
/// used to maintain the PPM-C escape count (the number of distinct symbols).
#[derive(Debug, Clone)]
struct PpmOrder {
    tree: Vec<u16>,
    distinct: Vec<u16>,
    seen: Vec<u8>,
    mask: usize,
    idx: usize,
}

impl PpmOrder {
    fn new(bits: u32) -> Self {
        let n = 1usize << bits;
        PpmOrder {
            tree: vec![0u16; n * 512],
            distinct: vec![0u16; n],
            seen: vec![0u8; n * 32],
            mask: n - 1,
            idx: 0,
        }
    }

    #[inline]
    fn set(&mut self, ctx: u32) {
        self.idx = (ctx as usize) & self.mask;
    }

    #[inline]
    fn mass(&self, p: usize) -> i64 {
        self.tree[self.idx * 512 + p] as i64
    }

    #[inline]
    fn total(&self) -> i64 {
        self.tree[self.idx * 512 + 1] as i64
    }

    #[inline]
    fn escapes(&self) -> i64 {
        self.distinct[self.idx] as i64
    }

    #[inline]
    fn update(&mut self, sym: u8) {
        let base = self.idx * 512;
        let s = sym as usize;
        let si = self.idx * 32 + (s >> 3);
        let bit = 1u8 << (s & 7);
        if self.seen[si] & bit == 0 {
            self.seen[si] |= bit;
            self.distinct[self.idx] = self.distinct[self.idx].saturating_add(1);
        }
        let mut node = 256 + s;
        while node >= 1 {
            let v = self.tree[base + node];
            self.tree[base + node] = v.saturating_add(1);
            node >>= 1;
        }
    }

    fn memory_bytes(&self) -> u64 {
        self.tree.len() as u64 * 2 + self.distinct.len() as u64 * 2 + self.seen.len() as u64
    }
}

/// Phase 6.8: a bounded PPM-C byte model exposed as a single mixer expert.
///
/// The blended symbol distribution is turned into a per-bit probability by
/// comparing the probability mass of the two subtrees under the current byte
/// prefix, so escape/backoff is applied at symbol level while the model still
/// speaks the bit-stream interface the mixer expects.
#[derive(Debug, Clone)]
pub struct PpmModel {
    /// `orders[k-1]` is order k. Order 0 is the uniform prior.
    orders: Vec<PpmOrder>,
}

impl PpmModel {
    pub fn new(max_order: usize) -> Self {
        let mut orders = Vec::new();
        for k in 1..=max_order {
            let bits = match k {
                1 => 8,
                2 => 16,
                3 => 15,
                _ => 15,
            };
            orders.push(PpmOrder::new(bits));
        }
        PpmModel { orders }
    }

    /// Select the context of every order from the buffer tail.
    pub fn set_contexts(&mut self, buf: &[u8]) {
        let n = buf.len();
        for (i, o) in self.orders.iter_mut().enumerate() {
            let k = (i + 1).min(n);
            let ctx = if k == 0 {
                0
            } else {
                hash_bytes(&buf[n - k..n])
            };
            o.set(ctx);
        }
    }

    /// Fold the byte just observed into every order's context.
    pub fn update_byte(&mut self, sym: u8) {
        for o in self.orders.iter_mut() {
            o.update(sym);
        }
    }

    /// Blended probability mass of the subtree under prefix node `p`.
    fn bmass(&self, p: usize) -> i64 {
        let depth = 31 - (p as u32).leading_zeros() as i64; // 0..=8
        let leaves = 1i64 << (8 - depth);
        let mut b = leaves * PPM_SCALE / 256;
        for o in self.orders.iter() {
            let t = o.total();
            if t == 0 {
                continue;
            }
            let esc = o.escapes();
            let c = o.mass(p);
            b = (c * PPM_SCALE + esc * b) / (t + esc);
        }
        b
    }

    /// `P(next bit = 1)`, in `[1, 4095]`, for the current prefix `p` (which is
    /// exactly the predictor's partial-byte register `c0`).
    #[inline]
    pub fn predict(&self, p: usize) -> i32 {
        let den = self.bmass(p);
        if den <= 0 {
            return 2048;
        }
        let num = self.bmass(2 * p + 1);
        ((num * 4096) / den).clamp(1, 4095) as i32
    }

    pub fn memory_bytes(&self) -> u64 {
        self.orders.iter().map(|o| o.memory_bytes()).sum()
    }

    /// Projected allocation for a model of the given maximum order, without
    /// building it. Used by the memory guard so the estimate is conservative.
    pub fn estimated_bytes(max_order: usize) -> u64 {
        let mut m = 0u64;
        for k in 1..=max_order {
            let bits = match k {
                1 => 8,
                2 => 16,
                _ => 15,
            };
            let n = 1u64 << bits;
            m += n * (512 * 2 + 2 + 32);
        }
        m
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
            CtxKind::StateOrder(_) => {
                parents[i] = last_order.or(order1).or(order0);
            }
            CtxKind::Sparse(_, _) => {
                parents[i] = last_order.or(order1).or(order0);
            }
            CtxKind::Indirect { .. } | CtxKind::IndirectCtl { .. } => {
                parents[i] = order1.or(order0);
            }
            CtxKind::Stem | CtxKind::StemCtl => {
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

/// Phase 6.4: which key the extra SSE/APM stage uses. The stage itself is the
/// same adaptive map; only the context that selects its table differs, so a
/// comparison isolates the information in the key from the act of adding a
/// calibration stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sse3Mode {
    /// Informative: the two most recent bytes (order-2 context).
    Order2,
    /// Negative control: the most recent byte plus an uncorrelated distant byte.
    /// Same table size and adaptation as `Order2`, but the second key byte is
    /// drawn from far enough back that it carries no adjacency information.
    Distant,
    /// Phase 6.3 (ISSE): the learned indirect state of the last byte context.
    Indirect,
    /// Phase 6.3 control: the same stage with the state forced to a constant.
    IndirectConst,
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
    /// Phase 6.4: an extra SSE stage keyed on the last two bytes (0 = off).
    pub apm3_ctx: usize,
    /// Phase 6.4: the key family used by that stage.
    pub apm3_mode: Sse3Mode,
    /// Phase 6.7: context-slot collision control (0 off, 1 checksum, 2 control).
    pub collision: u8,
    /// Phase 6.8: maximum PPM-C order (0 = off).
    pub ppm_order: usize,
    /// Phase 8: apply the embedded learned residual corrector.
    pub residual: bool,
    /// Phase 8.7 control: apply the corrector with permuted weights.
    pub residual_ctl: bool,
    /// Phase 8 training (research): hidden width; >0 builds the offline trainer.
    pub residual_hidden: usize,
    /// Phase 8 training (research): SGD learning rate.
    pub residual_lr: f32,
    pub mixer_lr: i32,
    pub apm1_ctx: usize,
    pub apm2_ctx: usize,
    /// Phase 9: APM adaptation shifts, selected by the high nibble of `tune`.
    /// `(7, 7, 7)` reproduces the pre-Phase-9 behaviour exactly.
    pub apm1_rate: u32,
    pub apm2_rate: u32,
    pub apm3_rate: u32,
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
            apm3_ctx: 0,
            apm3_mode: Sse3Mode::Order2,
            collision: 0,
            ppm_order: 0,
            residual: false,
            residual_ctl: false,
            residual_hidden: 0,
            residual_lr: 0.02,
            mixer_lr: 12,
            apm1_ctx: 4096,
            apm2_ctx: 65536,
            apm1_rate: 7,
            apm2_rate: 7,
            apm3_rate: 7,
            info: InfoMode::None,
        }
    }

    /// Phase 6.4: enable an extra SSE stage keyed on the last two bytes.
    pub fn with_sse3(mut self) -> Self {
        self.apm3_ctx = 1 << 16;
        self.apm3_mode = Sse3Mode::Order2;
        self
    }

    /// Phase 6.4 negative control: the same stage with an uncorrelated key.
    pub fn with_sse3_ctl(mut self) -> Self {
        self.apm3_ctx = 1 << 16;
        self.apm3_mode = Sse3Mode::Distant;
        self
    }

    /// Phase 6.3: an indirect (state-keyed) SSE stage.
    pub fn with_isse(mut self) -> Self {
        self.apm3_ctx = 1 << 16;
        self.apm3_mode = Sse3Mode::Indirect;
        self
    }

    /// Phase 6.3 control: the same stage with a constant key.
    pub fn with_isse_ctl(mut self) -> Self {
        self.apm3_ctx = 1 << 16;
        self.apm3_mode = Sse3Mode::IndirectConst;
        self
    }

    /// Phase 6.8: enable the bounded PPM-C byte model at the given max order.
    pub fn with_ppm(mut self, order: usize) -> Self {
        self.ppm_order = order;
        self
    }

    /// Phase 8: apply the embedded learned residual corrector. `ctl` selects the
    /// permuted-weight control (identical size and code, no learned signal).
    pub fn with_residual(mut self, ctl: bool) -> Self {
        self.residual = true;
        self.residual_ctl = ctl;
        self
    }

    /// Phase 8 (research): train a residual corrector of the given hidden width.
    pub fn with_residual_train(mut self, hidden: usize) -> Self {
        self.residual_hidden = hidden;
        self
    }

    /// Phase 8 (research): the trainer learning rate.
    pub fn with_residual_lr(mut self, lr: f32) -> Self {
        self.residual_lr = lr;
        self
    }

    /// Phase 6.1: append indirect (bit-history state) experts at the given orders.
    pub fn with_state_orders(mut self, orders: &[usize]) -> Self {
        let bits = self.specs[0].bits;
        for &o in orders {
            self.specs.push(ModelSpec {
                kind: CtxKind::StateOrder(o),
                bits,
                rate: 5,
            });
        }
        self
    }

    /// Phase 6.1 control: append the same number of *direct* order experts, so the
    /// archive effect of merely widening the mixer is separable.
    pub fn with_orders(mut self, orders: &[usize]) -> Self {
        let bits = self.specs[0].bits;
        for &o in orders {
            self.specs.push(ModelSpec {
                kind: CtxKind::Order(o),
                bits,
                rate: 5,
            });
        }
        self
    }

    /// Phase 6.1: swap existing direct order experts for state-map experts of the
    /// same order, keeping the mixer width identical.
    pub fn with_replaced_state_orders(mut self, orders: &[usize]) -> Self {
        for s in self.specs.iter_mut() {
            if let CtxKind::Order(o) = s.kind {
                if orders.contains(&o) {
                    s.kind = CtxKind::StateOrder(o);
                }
            }
        }
        self
    }

    /// Phase 6.7: enable checksum-verified context slots.
    pub fn with_collision(mut self, mode: u8) -> Self {
        self.collision = mode;
        self
    }

    /// Phase 6.5: append a sparse (gapped) byte-context expert: the `order`
    /// bytes ending `gap` bytes before the current position.
    pub fn with_sparse(mut self, order: usize, gap: usize) -> Self {
        let bits = self.specs[0].bits;
        self.specs.push(ModelSpec {
            kind: CtxKind::Sparse(order, gap),
            bits,
            rate: 5,
        });
        self
    }

    /// Phase 6.2/6.6: append an indirect context expert.
    pub fn with_indirect(mut self, order: usize, hist: usize, chained: bool) -> Self {
        let bits = self.specs[0].bits;
        self.specs.push(ModelSpec {
            kind: CtxKind::Indirect {
                order,
                hist,
                chained,
            },
            bits,
            rate: 5,
        });
        self
    }

    /// Phase 6.2 negative control: indirect machine with a permuted slot key.
    pub fn with_indirect_ctl(mut self, order: usize, hist: usize) -> Self {
        let bits = self.specs[0].bits;
        self.specs.push(ModelSpec {
            kind: CtxKind::IndirectCtl { order, hist },
            bits,
            rate: 5,
        });
        self
    }

    /// Phase 6.5 representation control: swap the direct expert of `order` for a
    /// sparse expert of the same order and gap, keeping mixer width identical.
    pub fn with_replaced_sparse(mut self, order: usize, gap: usize) -> Self {
        for s in self.specs.iter_mut() {
            if s.kind == CtxKind::Order(order) {
                s.kind = CtxKind::Sparse(order, gap);
            }
        }
        self
    }

    /// Phase 6.9: delete the direct order experts in `orders` (law 7: an expert
    /// that does not pay for itself in archive bytes is removed).
    pub fn without_orders(mut self, orders: &[usize]) -> Self {
        self.specs
            .retain(|s| !matches!(s.kind, CtxKind::Order(o) if orders.contains(&o)));
        self
    }

    /// Phase 6.10: append a word model. `raw_ctl` selects the raw-word control
    /// (a duplicate of the existing word expert) instead of the stem-folded one.
    pub fn with_stem_model(mut self, raw_ctl: bool) -> Self {
        let kind = if raw_ctl {
            CtxKind::StemCtl
        } else {
            CtxKind::Stem
        };
        let bits = self.specs[0].bits.min(22);
        self.specs.push(ModelSpec {
            kind,
            bits,
            rate: 5,
        });
        self
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

    /// A20/Phase 9: map a tuning byte to a runtime hyperparameter set. The low
    /// nibble selects the mixer learning rate (A20's ladder) and the high nibble
    /// selects the three APM adaptation shifts, so `tune < 16` reproduces the
    /// pre-Phase-9 behaviour exactly and the whole 0..=255 space is a searchable
    /// hyperparameter set carried in one header byte at zero binary cost.
    pub fn with_tune(mut self, tune: u8) -> Self {
        self.mixer_lr = MIXER_LRS[(tune & 15) as usize];
        // The high nibble selects the APM adaptation shifts, but that axis is
        // REJECTED at enwik9 and compiled out by default (see `APM_RATE_SETS`).
        #[cfg(feature = "apm-tune")]
        {
            let (r1, r2, r3) = APM_RATE_SETS[(tune >> 4) as usize];
            self.apm1_rate = r1;
            self.apm2_rate = r2;
            self.apm3_rate = r3;
        }
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
            match s.kind {
                // Phase 6.1: the state experts store one byte per slot.
                CtxKind::StateOrder(_) => m += 1u64 << s.bits,
                CtxKind::Order(_) => {
                    m += (1u64 << s.bits) * 2;
                    // Phase 6.7: the checksum array costs one byte per slot.
                    if self.collision != 0 {
                        m += 1u64 << s.bits;
                    }
                }
                _ => m += (1u64 << s.bits) * 2,
            }
            if matches!(
                s.kind,
                CtxKind::Indirect { .. } | CtxKind::IndirectCtl { .. }
            ) {
                m += (1u64 << s.bits.min(18)) * 8;
            }
        }
        for t in &self.matches {
            m += (1u64 << t.bits) * 4;
        }
        m += (self.apm1_ctx as u64) * 33 * 4;
        m += (self.apm2_ctx as u64) * 33 * 4;
        m += (self.apm3_ctx as u64) * 33 * 4;
        // Only charged when the PPM feature is compiled: otherwise the type is
        // absent from the submission build entirely.
        #[cfg(feature = "ppm")]
        if self.ppm_order > 0 {
            m += PpmModel::estimated_bytes(self.ppm_order);
        }
        m
    }
}

/// A20/Phase 9: the mixer learning-rate ladder indexed by the low nibble of the
/// `tune` byte. Exposed so search receipts can record the decoded rate.
pub const MIXER_LRS: [i32; 16] = [
    12, 4, 6, 8, 10, 16, 20, 24, 32, 40, 48, 64, 96, 128, 192, 256,
];

/// The APM adaptation shift the scored build uses.
///
/// With the Phase-9 `apm-tune` axis compiled out — the default, and the verdict
/// at enwik9 — this is the fixed pre-Phase-9 constant `7`, so [`Apm::new`] sees
/// a compile-time constant and no variable-rate plumbing survives into the
/// submission stub.
#[cfg(feature = "apm-tune")]
#[inline]
fn apm_rate(r: u32) -> u32 {
    r
}

#[cfg(not(feature = "apm-tune"))]
#[inline]
fn apm_rate(_r: u32) -> u32 {
    7
}

/// Phase 9: APM adaptation-shift sets selected by the high nibble of the `tune`
/// byte. Index 0 is `(7, 7, 7)` — the pre-Phase-9 behaviour, preserved exactly —
/// so `tune < 16` reproduces the frozen parent bit-for-bit.
///
/// **REJECTED at enwik9.** The axis has a strong dose-response at enwik7 (faster
/// adaptation is worth ≈12 KB on the mean) and flips sign at enwik8; measured at
/// the authority corpus, the best APM point at the best mixer LR is
/// **169,575,025** against **169,484,029** for APM-off, i.e. 90,996 B worse. The
/// axis therefore earns nothing and is compiled out of the scored build
/// (`--features apm-tune` reproduces the screening); the default build fixes all
/// three shifts at 7.
///
/// The table lives here, in the coding layer, and not in [`crate::search`]: the
/// decoder must select the same rates, so the constant is part of the
/// representation, while the search machinery around it is research-plane.
#[cfg(feature = "apm-tune")]
pub const APM_RATE_SETS: [(u32, u32, u32); 16] = [
    (7, 7, 7),
    (6, 6, 6),
    (5, 5, 5),
    (8, 8, 8),
    (6, 6, 5),
    (6, 5, 6),
    (5, 6, 6),
    (6, 6, 7),
    (6, 7, 6),
    (7, 6, 6),
    (5, 5, 6),
    (5, 6, 5),
    (6, 5, 5),
    (4, 5, 5),
    (5, 4, 5),
    (5, 5, 4),
];

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
    /// Phase 6.4: optional extra calibration stage (present when `apm3_ctx > 0`
    /// and the `sse-3` feature is compiled in).
    apm3: Option<Apm>,
    apm3_mode: Sse3Mode,
    /// Key for `apm3`, recomputed once per byte.
    sse_ctx: usize,
    /// Phase 6.3: learned state feeding the indirect SSE key.
    sse_state: Option<IndirectState>,
    /// Phase 6.8: bounded PPM-C byte model (one mixer expert).
    ppm: Option<PpmModel>,
    /// Phase 8: frozen learned residual corrector (T1/T2).
    #[cfg(feature = "learned")]
    residual: Option<crate::learned::Net>,
    /// Phase 8 (research): the offline trainer shadow.
    #[cfg(feature = "learned")]
    residual_trainer: Option<crate::learned::Trainer>,
    #[cfg(feature = "learned")]
    last_feats: crate::learned::Feats,
    #[cfg(feature = "learned")]
    last_s_pr: i32,
    st: StretchTable,
    buf: Vec<u8>,
    c0: u32,
    bitpos: u32,
    last_byte: u8,
    word_cur: u64,
    word_prev: u64,
    /// Phase 6.10: the raw lowercase bytes of the current word prefix (bounded).
    word_bytes: Vec<u8>,
    /// Start offset of the current line (after the most recent newline).
    line_start: usize,
    /// Start offset of the previous line.
    prev_line_start: usize,
    pr: i32,
    inputs: Vec<i32>,
    /// Phase 6.1: indirect (bit-history state) experts and the shared state map.
    smodels: Vec<StateModel>,
    sspecs: Vec<ModelSpec>,
    sctx: Vec<u32>,
    state_map: StateMap,
    /// Phase 6.2/6.6: per-spec indirect state machines (None for other kinds).
    indirect: Vec<Option<IndirectState>>,
}

impl Predictor {
    pub fn new(cfg: &ModelConfig, buf_capacity: usize) -> Self {
        let mut models: Vec<ContextModel> = Vec::new();
        let mut specs: Vec<ModelSpec> = Vec::new();
        let mut smodels: Vec<StateModel> = Vec::new();
        let mut sspecs: Vec<ModelSpec> = Vec::new();
        for s in &cfg.specs {
            if matches!(s.kind, CtxKind::StateOrder(_)) {
                smodels.push(StateModel::new(s.bits));
                sspecs.push(*s);
            } else {
                // Phase 6.7: collision control applies to the direct byte experts.
                let verify = if matches!(s.kind, CtxKind::Order(_)) {
                    cfg.collision
                } else {
                    0
                };
                models.push(ContextModel::new_verify(s.bits, s.rate, verify));
                specs.push(*s);
            }
        }
        let sspecs_len = sspecs.len();
        let specs_len = specs.len();
        let ppm_in = if cfg.ppm_order > 0 { 1 } else { 0 };
        let n_inputs =
            models.len() + sspecs_len + match_tiers(cfg).len() + cfg.rep_offsets + ppm_in;
        // Phase 6.2/6.6: build the indirect state machine for each indirect spec.
        let indirect: Vec<Option<IndirectState>> = specs
            .iter()
            .map(|s| match s.kind {
                CtxKind::Indirect {
                    order: _,
                    hist,
                    chained,
                } => Some(IndirectState::new(s.bits, hist, chained, false)),
                CtxKind::IndirectCtl { order: _, hist } => {
                    Some(IndirectState::new(s.bits, hist, false, true))
                }
                _ => None,
            })
            .collect();
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
            parent: compute_parents(&specs),
            info: cfg.info,
            specs,
            ctx: vec![0; specs_len],
            match_models,
            smodels,
            sspecs,
            sctx: vec![0; sspecs_len],
            state_map: StateMap::new(),
            indirect,
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
            // The APM adaptation shifts are a compile-time constant unless the
            // rejected `apm-tune` axis is compiled in, so the default scored
            // build carries no variable-rate plumbing.
            apm1: Apm::new(cfg.apm1_ctx, apm_rate(cfg.apm1_rate)),
            apm2: Apm::new(cfg.apm2_ctx, apm_rate(cfg.apm2_rate)),
            apm3: {
                #[cfg(feature = "sse-3")]
                {
                    if cfg.apm3_ctx > 0 {
                        Some(Apm::new(cfg.apm3_ctx, apm_rate(cfg.apm3_rate)))
                    } else {
                        None
                    }
                }
                #[cfg(not(feature = "sse-3"))]
                {
                    None
                }
            },
            apm3_mode: cfg.apm3_mode,
            sse_ctx: 0,
            sse_state: {
                #[cfg(feature = "isse")]
                {
                    if cfg.apm3_ctx > 0
                        && matches!(cfg.apm3_mode, Sse3Mode::Indirect | Sse3Mode::IndirectConst)
                    {
                        Some(IndirectState::new(16, 2, false, false))
                    } else {
                        None
                    }
                }
                #[cfg(not(feature = "isse"))]
                {
                    None
                }
            },
            ppm: {
                #[cfg(feature = "ppm")]
                {
                    if cfg.ppm_order > 0 {
                        Some(PpmModel::new(cfg.ppm_order))
                    } else {
                        None
                    }
                }
                #[cfg(not(feature = "ppm"))]
                {
                    None
                }
            },
            #[cfg(feature = "learned")]
            residual: {
                if cfg.residual {
                    crate::learned::load().map(|n| if cfg.residual_ctl { n.shuffled() } else { n })
                } else {
                    None
                }
            },
            #[cfg(feature = "learned")]
            residual_trainer: if cfg.residual_hidden > 0 {
                Some(crate::learned::Trainer::new(
                    cfg.residual_hidden,
                    cfg.residual_lr,
                ))
            } else {
                None
            },
            #[cfg(feature = "learned")]
            last_feats: crate::learned::Feats([0; crate::learned::NF]),
            #[cfg(feature = "learned")]
            last_s_pr: 0,
            st: StretchTable::new(),
            buf: Vec::with_capacity(buf_capacity),
            c0: 1,
            bitpos: 0,
            last_byte: 0,
            word_cur: 0,
            word_prev: 0,
            word_bytes: Vec::with_capacity(64),
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
                // State experts are handled by the second loop below.
                CtxKind::StateOrder(_) => 0,
                // Phase 6.5: sparse (gapped) byte context.
                CtxKind::Sparse(order, gap) => {
                    let need = order + gap;
                    if n >= need {
                        hash_bytes(&self.buf[n - need..n - gap])
                    } else {
                        0
                    }
                }
                // Phase 6.2/6.6: handled by the indirect loop below.
                CtxKind::Indirect { .. } | CtxKind::IndirectCtl { .. } => 0,
                // Phase 6.10: stem-folded word model, and its raw-word control.
                CtxKind::Stem => fold_bytes(stem_bytes(&self.word_bytes)) as u32,
                CtxKind::StemCtl => fold_bytes(&self.word_bytes) as u32,
            };
            self.ctx[i] = c;
        }
        // Phase 6.2/6.6: advance each indirect state machine and key its expert on
        // the pair (source slot, learned state).
        for i in 0..self.specs.len() {
            let kind = self.specs[i].kind;
            if matches!(kind, CtxKind::Indirect { .. } | CtxKind::IndirectCtl { .. }) {
                let order = match kind {
                    CtxKind::Indirect { order, .. } | CtxKind::IndirectCtl { order, .. } => order,
                    _ => 0,
                };
                let o = order.min(n);
                let src = hash_bytes(&self.buf[n - o..n]);
                if let Some(st) = self.indirect[i].as_mut() {
                    self.ctx[i] = st.refresh(&self.buf, src);
                }
            }
        }
        for (i, c) in self.ctx.iter().enumerate() {
            self.models[i].set_context(*c);
        }
        // Phase 6.1: state-model contexts use the same order semantics.
        for (i, spec) in self.sspecs.iter().enumerate() {
            let c = match spec.kind {
                CtxKind::StateOrder(o) => {
                    let o = o.min(n);
                    hash_bytes(&self.buf[n - o..n])
                }
                _ => 0,
            };
            self.sctx[i] = c;
        }
        for (i, c) in self.sctx.iter().enumerate() {
            self.smodels[i].set_context(*c);
        }
        // Phase 6.4: key for the extra calibration stage, recomputed once per
        // byte. `Order2` uses the two adjacent preceding bytes; `Distant` keeps
        // the last byte but replaces the second with one drawn from far enough
        // back that adjacency is destroyed.
        self.sse_ctx = if self.apm3.is_some() {
            let last = if n >= 1 { self.buf[n - 1] } else { 0 };
            match self.apm3_mode {
                Sse3Mode::Order2 => {
                    let second = if n >= 2 { self.buf[n - 2] } else { 0 };
                    hash_bytes(&[last, second]) as usize
                }
                Sse3Mode::Distant => {
                    const DIST: usize = 251;
                    let second = if n > DIST { self.buf[n - 1 - DIST] } else { 0 };
                    hash_bytes(&[last, second]) as usize
                }
                Sse3Mode::Indirect | Sse3Mode::IndirectConst => {
                    #[cfg(feature = "isse")]
                    {
                        let src = hash_bytes(&self.buf[n.saturating_sub(1)..n]);
                        match self.sse_state.as_mut() {
                            Some(st) => {
                                let v = st.refresh(&self.buf, src);
                                if matches!(self.apm3_mode, Sse3Mode::Indirect) {
                                    v as usize
                                } else {
                                    0
                                }
                            }
                            None => 0,
                        }
                    }
                    #[cfg(not(feature = "isse"))]
                    {
                        0
                    }
                }
            }
        } else {
            0
        };
        // Phase 6.8: select the PPM contexts for the next byte.
        if let Some(ppm) = self.ppm.as_mut() {
            ppm.set_contexts(&self.buf);
        }
    }

    /// Called after each byte is appended: update word state and match model.
    fn obs_core(&mut self) {
        if let Some(&b) = self.buf.last() {
            let lc = b.to_ascii_lowercase();
            if lc.is_ascii_alphabetic() || lc == b'\'' {
                self.word_cur = word_add(self.word_cur, lc);
                if self.word_bytes.len() < 64 {
                    self.word_bytes.push(lc);
                }
            } else {
                if self.word_cur != 0 {
                    self.word_prev = self.word_cur;
                }
                self.word_cur = 0;
                self.word_bytes.clear();
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
        // Phase 6.8: fold the observed byte into the PPM contexts selected for it
        // (which are still the ones from the previous `refresh_contexts`).
        if let Some(ppm) = self.ppm.as_mut() {
            if let Some(&b) = self.buf.last() {
                ppm.update_byte(b);
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
        // Phase 6.1: indirect (state-map) experts.
        let ns = self.smodels.len();
        for k in 0..ns {
            let st = self.smodels[k].begin(self.c0);
            self.inputs[nm + k] = self.state_map.predict(st, &self.st);
        }
        let mbase = nm + ns;
        for (k, mm) in self.match_models.iter_mut().enumerate() {
            self.inputs[mbase + k] = mm.predict(self.bitpos);
        }
        // Phase 4.3: repeat-offset predictors.
        let rep_base = mbase + self.match_models.len();
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

        // Phase 6.8: the PPM-C expert (a byte-level model rendered as one bit
        // expert via subtree masses).
        if let Some(ppm) = self.ppm.as_ref() {
            let pr = ppm.predict(self.c0 as usize);
            self.inputs[rep_base + self.rep.len()] = self.st.stretch(pr);
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
        let blended = (raw + a1 + 2 * a2 + 2) >> 2;
        // Phase 6.4: an additional calibration stage, interpolated into the
        // running prediction exactly as the PAQ APM chain does. The key is the
        // per-byte context combined with the partial byte `c0`, so each bit
        // position within the byte gets its own calibration buckets.
        #[cfg(feature = "sse-3")]
        let (pr0, a3v) = {
            let cxt = self.sse_ctx ^ (self.c0.wrapping_mul(MIX_C)) as usize;
            match self.apm3.as_mut() {
                Some(apm3) => {
                    let a3 = apm3.predict(blended, cxt);
                    ((3 * blended + a3 + 2) >> 2, a3)
                }
                None => (blended, blended),
            }
        };
        #[cfg(not(feature = "sse-3"))]
        let (pr0, a3v) = (blended, blended);
        #[cfg(not(feature = "learned"))]
        let _ = a3v;

        // Phase 8: the learned residual corrector produces a logit correction on
        // top of the classical probability. Its features are the classical
        // outputs themselves (the T2 cascade).
        #[cfg(feature = "learned")]
        let pr = {
            if self.residual.is_some() || self.residual_trainer.is_some() {
                let match_dir = {
                    let mut best = 0i32;
                    for k in 0..self.match_models.len() {
                        let v = self.inputs[mbase + k];
                        if v.abs() > best.abs() {
                            best = v;
                        }
                    }
                    best.signum()
                };
                let feats = crate::learned::extract(&crate::learned::Raw14 {
                    s_raw: self.st.stretch(raw),
                    s_a1: self.st.stretch(a1),
                    s_a2: self.st.stretch(a2),
                    s_a3: self.st.stretch(a3v),
                    s_pr: self.st.stretch(pr0),
                    mstate: mstate as i32,
                    match_dir,
                    bitpos: self.bitpos as i32,
                    word_open: word_open != 0,
                    rep_active: rep_active != 0,
                });
                let corr = match (self.residual_trainer.as_ref(), self.residual.as_ref()) {
                    (Some(tr), _) => tr.corr_for(&feats),
                    (None, Some(net)) => net.forward(&feats),
                    _ => 0,
                };
                self.last_feats = feats;
                self.last_s_pr = self.st.stretch(pr0);
                crate::learned::stretch_and_apply(&self.st, pr0, corr)
            } else {
                pr0
            }
        };
        #[cfg(not(feature = "learned"))]
        let pr = pr0;

        self.pr = pr.clamp(1, 4094);
        self.pr as u32
    }

    #[inline]
    pub fn update(&mut self, bit: u32) {
        // Phase 8: one SGD step of the residual trainer, before the model state
        // advances. Inference has no trainer, so this is a no-op on the scored
        // path.
        #[cfg(feature = "learned")]
        if let Some(tr) = self.residual_trainer.as_mut() {
            tr.step(&self.last_feats, self.last_s_pr, bit);
        }
        self.mixer.update(bit);
        self.apm1.update(bit);
        self.apm2.update(bit);
        #[cfg(feature = "sse-3")]
        {
            if let Some(apm3) = self.apm3.as_mut() {
                apm3.update(bit);
            }
        }
        for m in &mut self.models {
            m.update(bit);
        }
        // Phase 6.1: train the shared state map, then the state histories.
        for k in 0..self.smodels.len() {
            let st = self.smodels[k].cur;
            self.state_map.update(st, bit);
            self.smodels[k].update(bit);
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

    /// Phase 8 (research): the quantized network after offline training.
    #[cfg(feature = "learned")]
    pub fn take_residual_net(&self) -> Option<crate::learned::Net> {
        self.residual_trainer.as_ref().map(|t| t.quantize())
    }

    /// Phase 8 (research): training diagnostics.
    #[cfg(feature = "learned")]
    pub fn residual_stats(&self) -> (u64, f64, f64) {
        self.residual_trainer
            .as_ref()
            .map(|t| (t.steps, t.mean_loss_bits(), t.ema_bits))
            .unwrap_or((0, 0.0, 0.0))
    }

    pub fn memory_bytes(&self) -> u64 {
        let mut m: u64 = 0;
        for x in &self.models {
            m += x.memory_bytes();
        }
        for s in &self.smodels {
            m += s.memory_bytes();
        }
        m += self.state_map.memory_bytes();
        for st in self.indirect.iter().flatten() {
            m += st.memory_bytes();
        }
        if let Some(st) = &self.sse_state {
            m += st.memory_bytes();
        }
        if let Some(p) = &self.ppm {
            m += p.memory_bytes();
        }
        if let Some(a) = &self.apm3 {
            m += (a.ctx_count() as u64) * 33 * 4;
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

    /// Phase 8 (research): the trained residual network.
    #[cfg(feature = "learned")]
    pub fn take_residual_net(&self) -> Option<crate::learned::Net> {
        self.predictor.take_residual_net()
    }

    /// Phase 8 (research): training diagnostics.
    #[cfg(feature = "learned")]
    pub fn residual_stats(&self) -> (u64, f64, f64) {
        self.predictor.residual_stats()
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

    #[test]
    fn stem_strips_only_common_suffixes() {
        assert_eq!(stem_bytes(b"compression"), b"compression");
        assert_eq!(stem_bytes(b"compressed"), b"compress");
        assert_eq!(stem_bytes(b"compresses"), b"compress");
        assert_eq!(stem_bytes(b"running"), b"runn");
        assert_eq!(stem_bytes(b"quickly"), b"quick");
        // Short words and `ss` are protected.
        assert_eq!(stem_bytes(b"is"), b"is");
        assert_eq!(stem_bytes(b"class"), b"class");
    }

    #[test]
    fn sparse_context_skips_the_gap_byte() {
        let c = ModelConfig::for_size(1_000_000).with_sparse(4, 1);
        let spec = c.specs.last().unwrap();
        assert_eq!(spec.kind, CtxKind::Sparse(4, 1));
    }

    #[test]
    fn collision_control_cold_starts_a_mismatched_slot() {
        let st = StretchTable::new();
        // With a 2-slot table, context 1 and context 3 collide; the checksum must
        // make the second one start cold rather than inherit the first's counts.
        let mut m = ContextModel::new_verify(1, 4, 1);
        m.set_context(1);
        for _ in 0..800 {
            m.predict(1, &st, 32768);
            m.update(1);
        }
        let trained = m.predict(1, &st, 32768);
        assert!(trained > 1000, "did not train: p={trained}");
        m.set_context(3);
        let cold = m.predict(1, &st, 32768);
        assert!(cold.abs() < trained, "collided slot kept stale confidence");
    }

    #[test]
    fn collision_control_constant_matches_unverified() {
        // The cost-only control (`verify == 2`) must be bit-identical to the
        // parent model: it allocates checksums but never rejects a slot.
        let st = StretchTable::new();
        let mut a = ContextModel::new(16, 4);
        let mut b = ContextModel::new_verify(16, 4, 2);
        for i in 0..2000u32 {
            a.set_context(i & 63);
            b.set_context(i & 63);
            let pa = a.predict(i & 7, &st, 32768);
            let pb = b.predict(i & 7, &st, 32768);
            assert_eq!(pa, pb, "control diverged at i={i}");
            a.update(i & 1);
            b.update(i & 1);
        }
    }

    #[test]
    fn collision_verification_is_deterministic() {
        let st = StretchTable::new();
        let mut a = ContextModel::new_verify(10, 4, 1);
        let mut b = ContextModel::new_verify(10, 4, 1);
        for i in 0..4000u32 {
            let ctx = (i.wrapping_mul(7)) & 1023;
            a.set_context(ctx);
            b.set_context(ctx);
            assert_eq!(a.predict(i & 15, &st, 32768), b.predict(i & 15, &st, 32768));
            a.update(i & 1);
            b.update(i & 1);
        }
    }

    #[test]
    fn ppm_predicts_learned_continuations() {
        // After seeing "ab" many times in the order-2 context, the model must
        // predict `b` (0x62) more confidently than a uniform prior.
        let mut ppm = PpmModel::new(3);
        for _ in 0..50 {
            ppm.set_contexts(b"ab");
            ppm.update_byte(b'b');
        }
        ppm.set_contexts(b"ab");
        // Only `b` was ever seen after "ab", and its second bit is 1, so the
        // model must predict that bit with high confidence.
        let p1 = ppm.predict(2);
        assert!(p1 > 2500, "ppm did not learn: p1={p1}");
    }

    #[test]
    fn indirect_state_is_deterministic() {
        let mut a = IndirectState::new(16, 2, false, false);
        let mut b = IndirectState::new(16, 2, false, false);
        for i in 0..64u32 {
            let src = hash_bytes(&[i as u8]);
            let va = a.refresh(&[i as u8], src);
            let vb = b.refresh(&[i as u8], src);
            assert_eq!(va, vb);
        }
    }

    #[test]
    fn ppm_predict_conversion_is_consistent() {
        // A never-seen model must emit the neutral probability.
        let ppm = PpmModel::new(4);
        assert_eq!(ppm.predict(1), 2048);
    }
}
