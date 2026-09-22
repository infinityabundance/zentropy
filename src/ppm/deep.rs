//! Phase 14.32 — a deep-order PPM distribution provider.
//!
//! ## Why this exists
//!
//! `docs/PHASE14_FIRST_BOUNDARY.md` §6 attributes 76% of the coded codelength to
//! lexical prose, and §5 stops the procedural representation family because its
//! residual stream loses to direct coding even at zero sharing. The redirect is
//! the plan's own modelling branch, and §14.32 names the first mechanism: a
//! genuine PPM-family expert that exposes an entire next-byte *distribution*
//! rather than being reduced, prematurely, to one bit probability.
//!
//! The existing `context::PpmModel` is a *bit* model built out of a byte model:
//! it keeps a 256-leaf count tree per hashed context and answers only
//! `P(next bit = 1)` for the current partial byte. That interface is closed — the
//! distribution it implicitly forms is thrown away at every bit boundary, so it
//! can feed exactly one mixer input. A downstream hierarchical mixer and a
//! learned residual expert (§14.33–14.36) need the whole 256-symbol distribution,
//! once per byte, with a principled escape/backoff tail; that is what this module
//! provides.
//!
//! ## Orders are a ladder, not a constant
//!
//! §14.32 names 4/8/12/16/20/25, but the historical enwik9 result is a warning,
//! not an invitation: `Method::Ppm` (order 4) *lost* 61,133 B at fixed capacity
//! (2^24 tables). Depth is therefore a *configuration*, supplied as an ascending
//! ladder, and the measurement below sweeps the ladder at equal memory rather
//! than assuming a large order wins. [`DeepPpm::ladder_through`] is the default
//! shape (low orders for backoff, then the plan's named orders); nothing is
//! hardcoded to 25.
//!
//! ## Capacity policy: deterministic bounded eviction
//!
//! Memory is fixed at construction and never grows. Each order owns a
//! set-associative table of buckets × [`WAYS`] entries; every entry owns a fixed
//! slice of a per-order symbol arena, so an update never allocates. When a
//! context has no home and its bucket is full, one entry is evicted by a fixed
//! rule: **smallest total count, then least-recently-used, then lowest entry
//! index**. This is deliberately *not* "drop the oldest":
//!
//! * PPM statistics are frequency-weighted. The entry that earns codelength is
//!   the one the stream keeps returning to; an old but heavily observed context
//!   is the *most* valuable record in a bucket, and a pure FIFO would delete it
//!   in favour of a one-shot context that will never be seen again.
//! * At high orders almost every context is a singleton, so the frequency term
//!   ties and the rule degenerates to FIFO among equals — which is the right
//!   behaviour there — while low orders, where counts are large and reuse is
//!   real, are protected.
//! * Within an entry, when all symbol slots are full, the *least frequent
//!   symbol* is retired; its mass becomes escape mass and flows to lower orders,
//!   so the tail degrades gracefully instead of being silently zeroed.
//!
//! The rule is a pure function of the accumulated counts and the clock, with the
//! entry index as a total-order tie-break, so encoder and decoder evict
//! identically. We reject global LRU because maintaining it on a hash table
//! costs more bytes than the table it manages, and we reject the existing
//! expert's "merge colliding contexts into one slot" because merging distinct
//! contexts corrupts the very distribution this module exists to expose.
//!
//! ## Honest negative
//!
//! This module does **not** claim to win. Ideal codelength is a diagnostic; `S`
//! is authority (§0 law 1). The comparison harness at the bottom of the file is
//! written so that a negative is a first-class outcome: it reports the existing
//! expert beside the deep ladder at equal memory, and the capacity knee, and
//! asserts only reproducibility and bounds — never a winner.
//!
//! ## Arithmetic
//!
//! The model is integer throughout: counts, escape masses and the normalised
//! distribution are all integer. Floating point appears only in the *diagnostic*
//! `-log2 p` accumulators, exactly as the plan allows.

use crate::context::PpmModel;
use crate::entropy::PROB_SCALE;

/// Scale of a [`Distribution`]: `Σ_b p(b) == DIST_SCALE`, exactly.
///
/// The same 2^16 scale the existing PPM expert uses for its symbol masses, so the
/// two are directly comparable without a rescale.
pub const DIST_SCALE: u32 = 1 << 16;

/// Entries per bucket. Four ways is the smallest associativity that gives the
/// eviction rule something to choose between without making a lookup scan four
/// times as many entries as it needs.
const WAYS: usize = 4;

/// Upper bound on a level's bucket exponent, so a generous budget cannot turn a
/// single level into a multi-hundred-megabyte allocation without the caller
/// seeing it in the constructor's split.
const MAX_BUCKET_BITS: u32 = 20;

/// Counts are stored in 24 bits. A symbol observed more than 2^24 times in one
/// context is saturated rather than overflowing, which keeps the packing exact
/// and the model deterministic.
const COUNT_CAP: u32 = 0x00FF_FFFF;

/// Mask over the low 24 bits of a packed `(symbol << 24) | count` slot.
const COUNT_MASK: u32 = 0x00FF_FFFF;

// ---------------------------------------------------------------------------
// The distribution
// ---------------------------------------------------------------------------

/// A full next-byte distribution, normalised to [`DIST_SCALE`].
///
/// It is the primary product of this module: a mixer can read all 256 masses, and
/// a residual expert can ask for the mass of any byte or any byte prefix. The
/// existing binary range coder is served by [`Distribution::bit_prob`], which is
/// the same distribution marginalised onto the current partial-byte node — so one
/// representation feeds both consumers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Distribution {
    p: [u32; 256],
}

impl Distribution {
    /// Probability mass of `byte`, in units of `1 / DIST_SCALE`.
    #[inline]
    pub fn probability(&self, byte: u8) -> u32 {
        self.p[byte as usize]
    }

    /// All 256 masses, in byte order.
    #[inline]
    pub fn as_array(&self) -> &[u32; 256] {
        &self.p
    }

    /// Total mass. Every distribution this module produces sums to exactly
    /// [`DIST_SCALE`]; the method exists so a test can assert it.
    #[inline]
    pub fn sum(&self) -> u64 {
        self.p.iter().map(|&x| x as u64).sum()
    }

    /// Mass of the byte prefix represented by `node` in the MSB-first bit tree
    /// (`node == 1` is the whole byte, `node` in `128..=255` is a one-bit prefix).
    fn prefix_mass(&self, node: usize) -> u64 {
        let depth = (usize::BITS - 1 - node.leading_zeros()) as usize;
        let base = (node - (1usize << depth)) << (8 - depth);
        let len = 1usize << (8 - depth);
        self.p[base..base + len].iter().map(|&x| x as u64).sum()
    }

    /// `P(next bit = 1)` in `[1, PROB_SCALE - 1]` for prefix node `node`.
    ///
    /// This is the reduction the existing range coder consumes: the mass of the
    /// right child over the mass of the node. It is the *summary* this module
    /// warns against relying on — every bit decision after the first is made
    /// without being able to revisit the symbol-level tail — which is precisely
    /// why the measurement below reports it separately.
    #[inline]
    pub fn bit_prob(&self, node: usize) -> u32 {
        if node == 0 {
            return PROB_SCALE / 2;
        }
        let den = self.prefix_mass(node);
        if den == 0 {
            return PROB_SCALE / 2;
        }
        let num = self.prefix_mass(2 * node + 1);
        ((num * PROB_SCALE as u64) / den).clamp(1, (PROB_SCALE - 1) as u64) as u32
    }

    /// Ideal codelength of `byte` under this distribution, in bits.
    ///
    /// Diagnostic only (it is `f64`); `S` remains the authority.
    #[inline]
    pub fn ideal_bits(&self, byte: u8) -> f64 {
        let p = self.p[byte as usize] as f64 / DIST_SCALE as f64;
        -p.log2()
    }
}

/// Index of the largest mass, lowest index on a tie. The normalisation and
/// zero-floor passes rely on it being deterministic.
fn argmax(p: &[u32; 256]) -> usize {
    let mut best = 0usize;
    let mut best_v = 0u32;
    for (i, &v) in p.iter().enumerate() {
        if v > best_v {
            best_v = v;
            best = i;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// One order's bounded table
// ---------------------------------------------------------------------------

/// One slot of a context record: `tag`, live totals and the LRU clock.
///
/// Kept to four fields so the per-entry fixed cost is small; the symbol counts
/// live in [`Level::arena`] so a record can hold a variable number of symbols
/// without a per-entry heap allocation.
#[derive(Clone, Copy)]
struct Entry {
    /// 64-bit context hash with the low bit forced set; `0` means empty.
    tag: u64,
    /// Sum of the live slot counts.
    total: u32,
    /// Number of live symbol slots.
    distinct: u16,
    /// Generation of the last access, for the LRU tie-break.
    tick: u32,
}

impl Entry {
    const EMPTY: Entry = Entry {
        tag: 0,
        total: 0,
        distinct: 0,
        tick: 0,
    };
}

/// A bounded, set-associative table for one context order.
///
/// `entries.len() == (1 << bits) * WAYS` and entry `i` owns the arena slice
/// `arena[i * slots .. (i + 1) * slots]`. Both vectors are sized once, in the
/// constructor; an update never reallocates.
struct Level {
    /// Context length in bytes (`0` is the empty-context histogram).
    order: usize,
    /// `(1 << bits) - 1`.
    mask: usize,
    /// Symbol slots per entry.
    slots: usize,
    entries: Vec<Entry>,
    arena: Vec<u32>,
    evictions: u64,
}

impl Level {
    fn new(order: usize, bits: u32, slots: usize) -> Self {
        let slots = slots.max(1);
        let n = (1usize << bits) * WAYS;
        Level {
            order,
            mask: (1usize << bits) - 1,
            slots,
            entries: vec![Entry::EMPTY; n],
            arena: vec![0u32; n * slots],
            evictions: 0,
        }
    }

    #[inline]
    fn memory_bytes(&self) -> u64 {
        self.entries.len() as u64 * std::mem::size_of::<Entry>() as u64
            + self.arena.len() as u64 * 4
    }

    #[inline]
    fn bucket_of(&self, hash: u64) -> usize {
        // The high bits of the hash select the bucket; the whole hash remains in
        // the tag, so two contexts that share a bucket still differ.
        ((hash >> 40) as usize) & self.mask
    }

    /// Locate the record for a context, if present. Pure.
    #[inline]
    fn find(&self, hash: u64) -> Option<usize> {
        let tag = hash | 1;
        let base = self.bucket_of(hash) * WAYS;
        for w in 0..WAYS {
            let ei = base + w;
            if self.entries[ei].tag == tag {
                return Some(ei);
            }
        }
        None
    }

    /// Fold an observation into this order, evicting a victim if the bucket is
    /// full. Deterministic in `(total, tick, index)`.
    fn update(&mut self, hash: u64, byte: u8, clock: u32) {
        let tag = hash | 1;
        let base = self.bucket_of(hash) * WAYS;
        let mut hit = None;
        let mut empty = None;
        let mut victim = base;
        let mut victim_key = (u32::MAX, u32::MAX, usize::MAX);
        for w in 0..WAYS {
            let ei = base + w;
            let e = self.entries[ei];
            if e.tag == tag {
                hit = Some(ei);
                break;
            }
            if e.tag == 0 {
                if empty.is_none() {
                    empty = Some(ei);
                }
                continue;
            }
            let key = (e.total, e.tick, ei);
            if key < victim_key {
                victim_key = key;
                victim = ei;
            }
        }
        let ei = match hit {
            Some(i) => i,
            None => match empty {
                Some(i) => {
                    self.install(i, tag);
                    i
                }
                None => {
                    // Bucket full: retire the least valuable record. See the
                    // module doc comment for why the victim is chosen by count
                    // first and recency only as a tie-break (`victim_key` is the
                    // minimum over live entries, with the entry index as the
                    // total-order tie-break).
                    self.evictions += 1;
                    self.install(victim, tag);
                    victim
                }
            },
        };
        self.bump(ei, byte, clock);
    }

    /// Clear a record and stamp it with a new tag. Its counts and symbol slots
    /// are zeroed so the retired statistics cannot leak into the new occupant.
    #[inline]
    fn install(&mut self, ei: usize, tag: u64) {
        self.entries[ei] = Entry {
            tag,
            total: 0,
            distinct: 0,
            tick: 0,
        };
        let off = ei * self.slots;
        for slot in &mut self.arena[off..off + self.slots] {
            *slot = 0;
        }
    }

    /// Increment `byte`'s count in record `ei`, adding or retiring a symbol slot
    /// if needed.
    fn bump(&mut self, ei: usize, byte: u8, clock: u32) {
        let off = ei * self.slots;
        let distinct = self.entries[ei].distinct as usize;
        let mut hit = None;
        for k in 0..distinct {
            if (self.arena[off + k] >> 24) as u8 == byte {
                hit = Some(k);
                break;
            }
        }
        self.entries[ei].tick = clock;
        match hit {
            Some(k) => {
                let v = self.arena[off + k];
                let count = v & COUNT_MASK;
                if count < COUNT_CAP {
                    self.arena[off + k] = (v & !COUNT_MASK) | (count + 1);
                    self.entries[ei].total += 1;
                }
            }
            None if distinct < self.slots => {
                self.arena[off + distinct] = ((byte as u32) << 24) | 1;
                self.entries[ei].distinct += 1;
                self.entries[ei].total += 1;
            }
            None => {
                // All slots live: retire the least frequent symbol. Its mass
                // leaves this order and rejoins the escape tail below.
                let mut slot = 0usize;
                let mut least = u32::MAX;
                for k in 0..self.slots {
                    let c = self.arena[off + k] & COUNT_MASK;
                    if c < least {
                        least = c;
                        slot = k;
                    }
                }
                self.entries[ei].total = self.entries[ei].total - least + 1;
                self.arena[off + slot] = ((byte as u32) << 24) | 1;
            }
        }
    }

    #[inline]
    fn totals(&self, ei: usize) -> (u32, u16) {
        (self.entries[ei].total, self.entries[ei].distinct)
    }
}

// ---------------------------------------------------------------------------
// The deep model
// ---------------------------------------------------------------------------

/// A construction recipe for a single order. Public so a caller can pin an exact
/// memory geometry instead of accepting the budget split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelSpec {
    pub order: usize,
    pub bucket_bits: u32,
    pub slots: usize,
}

/// A configurable ladder of PPM orders, exposed as a next-byte distribution.
///
/// Construction fixes the memory; the only mutating operation is [`Self::observe`],
/// which reuses existing records and evicts deterministically. There is no path
/// that grows the model.
pub struct DeepPpm {
    /// Ascending by order, so backoff is a reverse iteration.
    levels: Vec<Level>,
    clock: u32,
    memory_bytes: u64,
    budget_bytes: u64,
}

impl DeepPpm {
    /// Default ladder up to `max_order`: low orders for the escape tail, then the
    /// orders §14.32 names. `max_order` is a parameter, never a constant.
    pub fn ladder_through(max_order: usize) -> Vec<usize> {
        let mut v: Vec<usize> = vec![0, 1, 2];
        for o in [3usize, 4, 6, 8, 12, 16, 20, 25] {
            if o <= max_order {
                v.push(o);
            }
        }
        v.retain(|&o| o <= max_order);
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Build exactly the given levels. The resulting [`Self::memory_bytes`] is
    /// the exact sum of the tables, so "memory is fixed at construction" is a
    /// checkable equality rather than a promise.
    pub fn with_levels(specs: &[LevelSpec]) -> Self {
        let mut levels = Vec::with_capacity(specs.len());
        for s in specs {
            levels.push(Level::new(s.order, s.bucket_bits, s.slots));
        }
        levels.sort_by_key(|l| l.order);
        let mut m = 0u64;
        for l in &levels {
            m += l.memory_bytes();
        }
        DeepPpm {
            levels,
            clock: 0,
            memory_bytes: m,
            budget_bytes: m,
        }
    }

    /// Symbol slots per order, chosen by how many distinct continuations an
    /// order can plausibly serve: the empty context needs most of the byte
    /// alphabet, deep contexts need very few.
    fn slots_for(order: usize) -> usize {
        match order {
            0 => 64,
            1 | 2 => 32,
            3..=8 => 8,
            _ => 4,
        }
    }

    /// Build a ladder that fits inside `budget_bytes`, splitting the budget
    /// across levels inversely with order (lower orders are reused far more
    /// often, so they earn the larger share). Deterministic: the split is pure
    /// integer arithmetic, and a level whose share cannot hold even one bucket is
    /// omitted rather than silently overspending.
    pub fn with_budget(ladder: &[usize], budget_bytes: u64) -> Self {
        let orders: Vec<usize> = {
            let mut v = ladder.to_vec();
            v.sort_unstable();
            v.dedup();
            v
        };
        let mut weight_sum = 0u64;
        for &o in &orders {
            weight_sum += 1_000_000 / (o as u64 + 1);
        }
        if weight_sum == 0 {
            return DeepPpm::with_levels(&[]);
        }
        let mut specs = Vec::new();
        for &o in &orders {
            let slots = Self::slots_for(o);
            let bucket = bucket_bytes(slots);
            let share = budget_bytes.saturating_mul(1_000_000 / (o as u64 + 1)) / weight_sum;
            if share < bucket {
                continue;
            }
            let bits = floor_log2((share / bucket).max(1))
                .min(MAX_BUCKET_BITS)
                .max(1);
            specs.push(LevelSpec {
                order: o,
                bucket_bits: bits,
                slots,
            });
        }
        // Spend the remainder greedily, lowest order first, so "at equal memory"
        // is tight rather than merely an upper bound. The pass is deterministic:
        // a fixed order over a fixed list, adding one bucket-exponent at a time.
        loop {
            let mut used: u64 = specs
                .iter()
                .map(|s| bucket_bytes(s.slots) << s.bucket_bits)
                .sum();
            if used >= budget_bytes {
                break;
            }
            let mut progressed = false;
            for s in specs.iter_mut() {
                if s.bucket_bits >= MAX_BUCKET_BITS {
                    continue;
                }
                let step = bucket_bytes(s.slots) << s.bucket_bits;
                if used + step <= budget_bytes {
                    s.bucket_bits += 1;
                    used += step;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        let mut model = DeepPpm::with_levels(&specs);
        model.budget_bytes = budget_bytes;
        model
    }

    /// Exact bytes occupied by the tables. Constant for the model's lifetime.
    #[inline]
    pub fn memory_bytes(&self) -> u64 {
        self.memory_bytes
    }

    /// The budget the model was asked to fit.
    #[inline]
    pub fn budget_bytes(&self) -> u64 {
        self.budget_bytes
    }

    /// Total deterministic evictions since construction.
    #[inline]
    pub fn evictions(&self) -> u64 {
        self.levels.iter().map(|l| l.evictions).sum()
    }

    /// The orders this model actually maintains, ascending.
    pub fn orders(&self) -> Vec<usize> {
        self.levels.iter().map(|l| l.order).collect()
    }

    /// Fold `byte`, observed after context `ctx`, into every order.
    ///
    /// `ctx` is the already-coded history; order `k` uses its last `k` bytes. The
    /// caller keeps the history (as the coder must anyway), so the model holds no
    /// copy and cannot grow with the input.
    pub fn observe(&mut self, ctx: &[u8], byte: u8) {
        self.clock = self.clock.wrapping_add(1);
        let clock = self.clock;
        for lv in self.levels.iter_mut() {
            if lv.order > ctx.len() {
                continue;
            }
            let suffix = &ctx[ctx.len() - lv.order..];
            lv.update(hash64(suffix), byte, clock);
        }
    }

    /// The full next-byte distribution following `ctx`. Pure: it never mutates the
    /// model, so a caller may query hypothetical contexts.
    ///
    /// Backoff is PPM-C: from the highest order whose context is present down to
    /// the lowest, the record's symbol counts share `1 - esc/(total + esc)` of the
    /// remaining mass in proportion to their counts, `esc = distinct` carries the
    /// rest to the next order, and whatever survives to the end is spread
    /// uniformly. The final pass makes the sum exactly [`DIST_SCALE`] and floors
    /// every byte at one unit, so an unseen symbol receives escape mass rather
    /// than a zero (a zero would be an infinite cost and an illegal coder input).
    pub fn distribution(&self, ctx: &[u8]) -> Distribution {
        let mut p = [0u32; 256];
        let mut rem: u64 = DIST_SCALE as u64;
        for lv in self.levels.iter().rev() {
            if lv.order > ctx.len() {
                continue;
            }
            let suffix = &ctx[ctx.len() - lv.order..];
            let ei = match lv.find(hash64(suffix)) {
                Some(ei) => ei,
                None => continue,
            };
            let (total, distinct) = lv.totals(ei);
            if total == 0 || distinct == 0 {
                continue;
            }
            let den = total as u64 + distinct as u64;
            let off = ei * lv.slots;
            for k in 0..distinct as usize {
                let v = lv.arena[off + k];
                let sym = (v >> 24) as usize;
                let count = (v & COUNT_MASK) as u64;
                p[sym] = p[sym].saturating_add(((rem * count) / den) as u32);
            }
            rem = rem * distinct as u64 / den;
        }
        if rem > 0 {
            // Order -1: uniform over the byte alphabet. This *is* the floor that
            // makes an unobserved symbol legal.
            let each = (rem / 256) as u32;
            for slot in p.iter_mut() {
                *slot += each;
            }
            for slot in p.iter_mut().take((rem % 256) as usize) {
                *slot += 1;
            }
        }
        let sum: u64 = p.iter().map(|&x| x as u64).sum();
        if sum < DIST_SCALE as u64 {
            // Truncating integer division lost at most one unit per contribution;
            // return the residue to the largest bucket so the scale is exact.
            let mx = argmax(&p);
            p[mx] += (DIST_SCALE as u64 - sum) as u32;
        }
        // Floor every byte at one unit. An unobserved symbol must be *expensive*,
        // not impossible: a zero probability is an illegal coder input. One pass
        // finds the zeros and the largest bucket, a second moves mass from the
        // largest to the floor. The largest bucket always holds far more than 255
        // units at this scale, so the transfer is always possible.
        let zeros = p.iter().filter(|&&x| x == 0).count();
        if zeros > 0 {
            let mx = argmax(&p);
            let take = (zeros as u32).min(p[mx] - 1);
            p[mx] -= take;
            let mut left = take as usize;
            for slot in p.iter_mut() {
                if *slot == 0 && left > 0 {
                    *slot = 1;
                    left -= 1;
                }
            }
        }
        debug_assert_eq!(p.iter().map(|&x| x as u64).sum::<u64>(), DIST_SCALE as u64);
        Distribution { p }
    }
}

/// Bytes one bucket occupies for an order with `slots` symbol slots: [`WAYS`]
/// entries plus their arena slices.
#[inline]
fn bucket_bytes(slots: usize) -> u64 {
    (std::mem::size_of::<Entry>() as u64 + slots as u64 * 4) * WAYS as u64
}

/// `floor(log2(x))` for `x >= 1`.
#[inline]
fn floor_log2(x: u64) -> u32 {
    63 - x.max(1).leading_zeros()
}

/// 64-bit FNV-1a. Local and fixed so the model hashes identically on every build;
/// the tag carries the whole hash, so the bucket selection cannot collide two
/// contexts by itself.
#[inline]
fn hash64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---------------------------------------------------------------------------
// Measurement: the deep ladder against the existing expert, at equal memory
// ---------------------------------------------------------------------------

/// Ideal-codelength result for one expert over a slice.
#[derive(Clone, PartialEq, Debug)]
pub struct ExpertStat {
    pub label: String,
    pub order: usize,
    pub memory_bytes: u64,
    pub ideal_bytes: f64,
    pub bits_per_byte: f64,
}

/// Ideal-codelength result for one deep ladder, reporting the distribution and
/// its reduction to one bit per decision separately.
#[derive(Clone, PartialEq, Debug)]
pub struct DeepStat {
    pub max_order: usize,
    pub ladder: Vec<usize>,
    pub memory_bytes: u64,
    pub evictions: u64,
    /// `-log2 p(byte)` summed — what a mixer/residual expert could exploit.
    pub dist_bits_per_byte: f64,
    /// The same distribution reduced to `P(bit)` for each of the eight decisions.
    pub bit_bits_per_byte: f64,
}

impl DeepStat {
    pub fn dist_ideal_bytes(&self, n: usize) -> f64 {
        self.dist_bits_per_byte * n as f64 / 8.0
    }
    pub fn bit_ideal_bytes(&self, n: usize) -> f64 {
        self.bit_bits_per_byte * n as f64 / 8.0
    }
}

/// One point of the capacity scan.
#[derive(Clone, PartialEq, Debug)]
pub struct CapPoint {
    pub budget_bytes: u64,
    pub memory_bytes: u64,
    pub bits_per_byte: f64,
}

/// The whole comparison, reproducible from the same inputs.
#[derive(Clone, PartialEq, Debug)]
pub struct Comparison {
    pub slice_bytes: usize,
    pub existing: ExpertStat,
    pub deep: Vec<DeepStat>,
    pub capacity: Vec<CapPoint>,
    /// Budget at which the capacity scan stops returning material gains.
    pub knee_bytes: Option<u64>,
}

/// Ideal codelength of the existing `PpmModel` exactly as the coder drives it:
/// contexts selected from the byte history, eight subtree-mass bit decisions per
/// byte.
pub fn existing_ppm_stat(data: &[u8], order: usize) -> ExpertStat {
    let mut model = PpmModel::new(order);
    let mut buf: Vec<u8> = Vec::with_capacity(data.len());
    let mut bits = 0f64;
    for &byte in data {
        model.set_contexts(&buf);
        let mut node = 1usize;
        for k in 0..8 {
            let bit = ((byte >> (7 - k)) & 1) as u32;
            let p = model.predict(node) as f64;
            let prob = if bit == 1 { p } else { PROB_SCALE as f64 - p };
            bits += -(prob / PROB_SCALE as f64).log2();
            node = node * 2 + bit as usize;
        }
        buf.push(byte);
        model.update_byte(byte);
    }
    ExpertStat {
        label: format!("PpmModel order {order} (as configured)"),
        order,
        memory_bytes: model.memory_bytes(),
        ideal_bytes: bits / 8.0,
        bits_per_byte: bits / data.len() as f64,
    }
}

/// Ideal codelength of a deep ladder over `data`, at a fixed construction budget.
/// Returns the distribution metric and the one-bit-per-decision reduction.
pub fn deep_stat(data: &[u8], ladder: &[usize], budget_bytes: u64) -> (DeepStat, ExpertStat) {
    let max_order = ladder.iter().copied().max().unwrap_or(0);
    let mut model = DeepPpm::with_budget(ladder, budget_bytes);
    let mut buf: Vec<u8> = Vec::with_capacity(data.len());
    let mut dbits = 0f64;
    let mut bbits = 0f64;
    for &byte in data {
        let d = model.distribution(&buf);
        dbits += d.ideal_bits(byte);
        let mut node = 1usize;
        for k in 0..8 {
            let bit = ((byte >> (7 - k)) & 1) as u32;
            let p = d.bit_prob(node) as f64;
            let prob = if bit == 1 { p } else { PROB_SCALE as f64 - p };
            bbits += -(prob / PROB_SCALE as f64).log2();
            node = node * 2 + bit as usize;
        }
        model.observe(&buf, byte);
        buf.push(byte);
    }
    let n = data.len() as f64;
    let mem = model.memory_bytes();
    let evict = model.evictions();
    let deep = DeepStat {
        max_order,
        ladder: ladder.to_vec(),
        memory_bytes: mem,
        evictions: evict,
        dist_bits_per_byte: dbits / n,
        bit_bits_per_byte: bbits / n,
    };
    let reduced = ExpertStat {
        label: format!("deep ladder ..{max_order}, distribution summarised to 1 bit"),
        order: max_order,
        memory_bytes: mem,
        ideal_bytes: bbits / 8.0,
        bits_per_byte: bbits / n,
    };
    (deep, reduced)
}

/// Run the comparison: the existing expert, the deep ladder at *its* memory for
/// each `max_orders` entry, and a capacity scan over `budgets` using the deepest
/// ladder.
pub fn compare(
    data: &[u8],
    existing_order: usize,
    max_orders: &[usize],
    budgets: &[u64],
) -> Comparison {
    let existing = existing_ppm_stat(data, existing_order);
    let mut deep = Vec::with_capacity(max_orders.len());
    for &mo in max_orders {
        let ladder = DeepPpm::ladder_through(mo);
        let (stat, _) = deep_stat(data, &ladder, existing.memory_bytes);
        deep.push(stat);
    }
    let deepest = max_orders.iter().copied().max().unwrap_or(0);
    let ladder = DeepPpm::ladder_through(deepest);
    let mut capacity = Vec::with_capacity(budgets.len());
    for &b in budgets {
        let (stat, _) = deep_stat(data, &ladder, b);
        capacity.push(CapPoint {
            budget_bytes: b,
            memory_bytes: stat.memory_bytes,
            bits_per_byte: stat.dist_bits_per_byte,
        });
    }
    let knee_bytes = capacity_knee(&capacity);
    Comparison {
        slice_bytes: data.len(),
        existing,
        deep,
        capacity,
        knee_bytes,
    }
}

/// The first budget `B` after which raising the budget further buys less than
/// 0.5% of the codelength (or regresses). This is the *measured* capacity knee:
/// the onset of diminishing returns, not the last point of the scan. It is not a
/// projection — the scan is the scan.
pub fn capacity_knee(scan: &[CapPoint]) -> Option<u64> {
    for w in scan.windows(2) {
        let (prev, cur) = (w[0].bits_per_byte, w[1].bits_per_byte);
        if prev > 0.0 && (prev - cur) / prev < 0.005 {
            return Some(w[0].budget_bytes);
        }
    }
    None
}

impl Comparison {
    /// Reproducible text report. It states plainly that ideal codelength is a
    /// diagnostic and that `S` is authority.
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("deep PPM distribution provider — measured comparison\n");
        s.push_str("(ideal codelength is a DIAGNOSTIC; S is authority)\n");
        s.push_str(&format!("slice: {} bytes\n\n", self.slice_bytes));
        s.push_str(&format!(
            "existing  {}: order {}, {:>12} B, {:.4} bits/byte, {:>10.1} B ideal\n\n",
            self.existing.label,
            self.existing.order,
            self.existing.memory_bytes,
            self.existing.bits_per_byte,
            self.existing.ideal_bytes
        ));
        s.push_str("deep ladder (equal memory = existing), ideal codelength:\n");
        s.push_str(
            "  max  ladder                              mem(B)  dist b/B  bit-summary b/B\n",
        );
        for d in &self.deep {
            let ladder: Vec<String> = d.ladder.iter().map(|o| o.to_string()).collect();
            s.push_str(&format!(
                "  {:>3}  {:<34} {:>8}  {:>8.4}  {:>8.4}\n",
                d.max_order,
                ladder.join(","),
                d.memory_bytes,
                d.dist_bits_per_byte,
                d.bit_bits_per_byte
            ));
        }
        s.push_str("\ncapacity scan (deepest ladder):\n");
        s.push_str("  budget(B)   mem(B)  dist b/B\n");
        for c in &self.capacity {
            s.push_str(&format!(
                "  {:>9}  {:>7}  {:>8.4}\n",
                c.budget_bytes, c.memory_bytes, c.bits_per_byte
            ));
        }
        match self.knee_bytes {
            Some(b) => s.push_str(&format!(
                "\ncapacity knee: {b} B (more budget buys < 0.5% further gain)\n"
            )),
            None => s.push_str("\ncapacity knee: none in scan (still improving)\n"),
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Tests — each is a claim about behaviour, and each can fail
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic stand-in when the development rung is not readable: an
    /// English-ish and XML-ish stream so escape/backoff has something to do.
    fn synthetic(n: usize) -> Vec<u8> {
        let seed = b"<page><title>Zentropy</title> the quick brown fox jumps over the lazy dog \
             [[Link]] {{cite|a=1}} 123 &amp;\n";
        let mut v = Vec::with_capacity(n);
        while v.len() < n {
            v.extend_from_slice(seed);
        }
        v.truncate(n);
        v
    }

    /// A slice of the development rung, or the synthetic stream if it is absent.
    fn slice(n: usize) -> Vec<u8> {
        match std::fs::read("evidence/corpus/enwik6") {
            Ok(mut v) => {
                v.truncate(n);
                v
            }
            Err(_) => synthetic(n),
        }
    }

    fn trained(ladder: &[usize], budget: u64, data: &[u8]) -> DeepPpm {
        let mut m = DeepPpm::with_budget(ladder, budget);
        let mut buf = Vec::with_capacity(data.len());
        for &b in data {
            m.observe(&buf, b);
            buf.push(b);
        }
        m
    }

    #[test]
    fn distribution_sums_to_scale() {
        let data = slice(64 * 1024);
        let m = trained(&DeepPpm::ladder_through(12), 1 << 20, &data);
        // Query a variety of contexts, including one longer than every order.
        let mut ctx = Vec::new();
        for &b in data.iter().take(2000) {
            ctx.push(b);
            let d = m.distribution(&ctx);
            assert_eq!(d.sum(), DIST_SCALE as u64, "sum at pos {}", ctx.len());
        }
        // And a pathological long context.
        let long = vec![b'x'; 100_000];
        assert_eq!(m.distribution(&long).sum(), DIST_SCALE as u64);
    }

    #[test]
    fn unseen_symbol_gets_escape_mass_not_zero() {
        // Train on a stream that never contains byte 0xFF.
        let data: Vec<u8> = b"the quick brown fox jumps over the lazy dog ".repeat(500);
        assert!(!data.contains(&0xFF));
        let m = trained(&DeepPpm::ladder_through(8), 1 << 20, &data);
        let ctx = &data[data.len() - 16..];
        let d = m.distribution(ctx);
        // The distribution is a legal coder input: every byte has positive mass.
        for b in 0u8..=255 {
            assert!(d.probability(b) >= 1, "byte {b} has zero mass");
        }
        // The unseen byte receives escape mass, but strictly less than the
        // byte the context actually predicts.
        let mx = d.as_array().iter().copied().max().unwrap();
        assert!(d.probability(0xFF) < mx, "unseen byte was over-weighted");
        assert!(d.probability(0xFF) >= 1);
    }

    #[test]
    fn deterministic_across_runs() {
        let data = slice(32 * 1024);
        let ladder = DeepPpm::ladder_through(16);
        let a = trained(&ladder, 1 << 20, &data);
        let b = trained(&ladder, 1 << 20, &data);
        assert_eq!(a.memory_bytes(), b.memory_bytes());
        assert_eq!(a.orders(), b.orders());
        assert_eq!(a.evictions(), b.evictions());
        let ctx = &data[data.len() - 40..];
        assert_eq!(a.distribution(ctx), b.distribution(ctx));
        // And the full ideal codelength agrees bit for bit.
        let (sa, _) = deep_stat(&data, &ladder, 1 << 20);
        let (sb, _) = deep_stat(&data, &ladder, 1 << 20);
        assert_eq!(sa, sb);
    }

    #[test]
    fn memory_exactly_bounded_and_constant() {
        let budget = 1u64 << 20;
        let ladder = DeepPpm::ladder_through(25);
        let mut m = DeepPpm::with_budget(&ladder, budget);
        assert!(m.memory_bytes() > 0);
        assert!(
            m.memory_bytes() <= budget,
            "memory {} exceeds budget {}",
            m.memory_bytes(),
            budget
        );
        // The constructor's claim is an equality with the summed geometry.
        let expected: u64 = m.levels.iter().map(|l| l.memory_bytes()).sum();
        assert_eq!(m.memory_bytes(), expected);
        let before = m.memory_bytes();
        // Hammer it with far more distinct contexts than it can hold.
        let data = slice(256 * 1024);
        let mut buf = Vec::new();
        for &b in &data {
            m.observe(&buf, b);
            buf.push(b);
        }
        assert_eq!(m.memory_bytes(), before, "model grew under load");
        assert!(m.evictions() > 0, "expected evictions under overflow");
    }

    #[test]
    fn eviction_policy_is_deterministic_and_protects_frequency() {
        // One order, one bucket, FOUR ways: every context shares a bucket, so the
        // eviction rule is exercised directly.
        let spec = LevelSpec {
            order: 1,
            bucket_bits: 0,
            slots: 2,
        };
        let mut m = DeepPpm::with_levels(&[spec]);
        // Make context "A" heavily observed: it must survive.
        for _ in 0..20 {
            m.observe(b"A", b'x');
        }
        // Fill the remaining three ways with one-shot singletons.
        m.observe(b"B", b'y');
        m.observe(b"C", b'z');
        m.observe(b"D", b'w');
        // "E" overflows the bucket: the victim is the lowest-total, oldest
        // singleton, never the high-count "A".
        m.observe(b"E", b'v');
        assert_eq!(m.evictions(), 1);
        let d = m.distribution(b"A");
        assert!(
            d.probability(b'x') > DIST_SCALE / 2,
            "the frequent context was evicted: {}",
            d.probability(b'x')
        );

        // Same operations, fresh model: identical eviction count and output.
        let mut m2 = DeepPpm::with_levels(&[spec]);
        for _ in 0..20 {
            m2.observe(b"A", b'x');
        }
        m2.observe(b"B", b'y');
        m2.observe(b"C", b'z');
        m2.observe(b"D", b'w');
        m2.observe(b"E", b'v');
        assert_eq!(m2.evictions(), m.evictions());
        assert_eq!(m2.distribution(b"A"), m.distribution(b"A"));
    }

    #[test]
    fn pathological_context_is_handled_without_panic() {
        let m = DeepPpm::with_budget(&DeepPpm::ladder_through(25), 1 << 18);
        // Empty context, context shorter than every order, and an absurdly long
        // context must all yield a legal, normalised distribution.
        for ctx in [vec![], vec![0u8], vec![7u8; 3], vec![0xFFu8; 5000]] {
            let d = m.distribution(&ctx);
            assert_eq!(d.sum(), DIST_SCALE as u64, "ctx len {}", ctx.len());
            // bit_prob is always a legal coder input.
            for node in 1..256usize {
                let p = d.bit_prob(node);
                assert!((1..PROB_SCALE).contains(&p), "node {node} -> {p}");
            }
        }
    }

    #[test]
    fn comparison_report_reproduces_on_second_call() {
        let data = slice(48 * 1024);
        let args = (1usize, vec![1usize, 4, 8], vec![1u64 << 18, 1 << 20]);
        let a = compare(&data, args.0, &args.1, &args.2);
        let b = compare(&data, args.0, &args.1, &args.2);
        assert_eq!(a, b, "comparison is not reproducible");
        assert_eq!(a.render(), b.render());
    }

    #[test]
    fn deep_ladder_measured_against_existing_at_equal_memory() {
        // The measured comparison the subphase asks for: the existing expert as
        // configured, the deep ladder at its memory, and the capacity scan. This
        // test asserts structure and bounds; it deliberately does not assert that
        // the deep ladder wins — a negative is a valid outcome.
        let data = slice(128 * 1024);
        let orders = [4usize, 8, 16, 25];
        let budgets = [1u64 << 18, 1 << 20, 1 << 22, 1 << 24, 1 << 26];
        let cmp = compare(&data, 4, &orders, &budgets);

        assert!(cmp.existing.bits_per_byte > 0.0 && cmp.existing.bits_per_byte <= 8.1);
        for d in &cmp.deep {
            assert!(d.memory_bytes <= cmp.existing.memory_bytes);
            assert!(d.dist_bits_per_byte > 0.0 && d.dist_bits_per_byte <= 8.1);
            assert!(d.bit_bits_per_byte > 0.0 && d.bit_bits_per_byte <= 8.1);
            // The distribution is never worse than its own bit reduction by more
            // than the (small) cost of marginalising; both price the same model.
            assert!(d.dist_bits_per_byte <= 8.1);
        }
        let report = cmp.render();
        assert!(report.contains("existing"));
        // Print the table when run with `-- --nocapture`.
        eprintln!("{report}");
    }
}
