//! A reusable **context map**: families of specialists from one mechanism.
//!
//! Phase 14's attribution (`docs/PHASE14_FIRST_BOUNDARY.md` §6) leaves the field
//! with one pool worth attacking: *better modelling of expensive running text*.
//! Lexical prose is 76% of the coded codelength there, and the model is already
//! 2× below order-0 on it, so the remaining work is not more structural
//! reconstruction — the procedural representation family was measured and
//! rejected (§4-5). It is a better estimator over the same bytes.
//!
//! The direct [`crate::context::ContextModel`] allocates one `u16` probability
//! per hashed slot. That is fast, but it has three properties this primitive is
//! built to change:
//!
//! 1. **A hash collision is invisible.** Two contexts that index the same slot
//!    silently share (and average) one probability. The map stores a 16-bit
//!    **tag** beside each probability, so a query finds *this* context or finds
//!    nothing — a collision becomes a cold start, not a blend of unrelated
//!    statistics.
//! 2. **The *what* and the *how much* are conflated.** A direct table adapts at
//!    one fixed shift for all contexts regardless of how much evidence a slot
//!    holds. Each slot here carries a **count** as well as its probability, so
//!    the map can offer a *stationary* estimator (a count-scaled step that
//!    converges on a fixed source) beside the *nonstationary* fixed-shift one.
//! 3. **There is no way to tell a live slot from a dead one.** Each slot stores a
//!    **recency stamp**, and a full bucket evicts its least-recently-used slot,
//!    so a context whose statistics have gone stale is displaced rather than
//!    kept forever.
//!
//! Buckets are **set-associative**: `sets × assoc` slots, hashed to a set and
//! probed across the set's ways. Associativity is the collision budget: `assoc`
//! distinct contexts can share one set without aliasing each other.
//!
//! # The point is the interface, not the table
//!
//! `p(ctx)` / `update(ctx, bit)` are a pure `key -> bit` estimator. A *caller*
//! supplies the key. An order-N specialist keys on the last N bytes folded with
//! the partial-byte tree node; a word specialist keys on a word hash; a
//! structural or match specialist keys on whatever causal state it owns. This
//! file deliberately builds no specialists of its own: it is the mechanism from
//! which a family of them can be instantiated at one cost, rather than one more
//! hand-tuned probability table per idea.
//!
//! # Honesty
//!
//! The probability is an **8-bit** state (`p / 256`), which is coarser than the
//! direct expert's 16-bit table, while a slot also carries a tag, a count, a
//! last-bit/run byte and a stamping word — 12 bytes against the direct table's 2.
//! At *equal memory* the map therefore owns ~6× fewer slots. That is the price of
//! collision detection and confidence tracking, and it is real. Whether the map
//! wins at equal memory is a measurement, not an argument, and it is reported
//! honestly by [`bench_against_direct`] — **including when it loses**. There is
//! no tuning of that comparison toward a desired answer.
//!
//! # Measured outcome (enwik6 slice, first 256 KiB, 1 MiB budget)
//!
//! It **loses overall**: cheaper on 10 of 36 comparison rows at approximately
//! equal memory, with the direct expert cheaper on the other 26 (and the map
//! holding 25% *less* memory, so the incumbent is not short-changed). The losses
//! are concentrated at the low orders (0–2), where 6× more slots and 16-bit
//! probabilities beat an 8-bit state outright, and on every `Fast` row below
//! order 3. The wins are real but narrow: in `Stationary` mode, at orders 3, 4
//! and 6, the count-scaled estimator with tagged slots is **19–35 kB cheaper**
//! across every associativity.
//!
//! So the mechanism is *not* a drop-in replacement for the direct expert, and
//! this file makes no such claim. It says the stationary estimator plus collision
//! detection has measurable value at higher orders, and that the 12-byte slot
//! cannot pay for itself at low ones. Ideal codelength is a **diagnostic**; `S`
//! (the fully charged submission) is authority (§0, law 1). Nothing here is
//! adopted, and this file changes no scored behaviour.

use crate::entropy::PROB_SCALE;

/// Smallest expressible set count (2^4 = 16 sets). Below this the tag has
/// almost nothing to discriminate and the map degenerates into a cache.
pub const MIN_BITS: u32 = 4;
/// Largest expressible set count (2^26). The largest legal table is then
/// bounded by [`MAX_BYTES`] anyway; this only keeps the shift well-defined.
pub const MAX_BITS: u32 = 26;
/// Largest associativity. A set is probed linearly, so this is also the worst
/// case probe cost and must stay small.
pub const MAX_ASSOC: usize = 8;
/// Hard ceiling on the table (1 GiB). Construction refuses to exceed it rather
/// than asking the allocator and hoping. This also keeps any test fixture inside
/// the test-plane ceiling in [`crate::memory`].
pub const MAX_BYTES: u64 = 1 << 30;
/// Fast-mode shifts are clamped to this range; a shift of 0 never moves and a
/// very large shift never adapts, so both are rejected.
pub const MIN_RATE: u32 = 1;
pub const MAX_RATE: u32 = 12;
/// The count saturates here. Keeping the stationary denominator bounded
/// (`count + 2`) keeps the step from rounding to zero as well as bounded below.
pub const COUNT_CAP: u8 = 60;
/// Neutral cold start: P(1) = 128/256 = 1/2.
const DEFAULT_P8: u8 = 128;
/// The fixed nonstationary shift used when a caller does not override it. It
/// matches the low-order direct experts' rate in `ModelConfig::for_size`.
pub const DEFAULT_FAST_RATE: u32 = 4;

/// Which adaptation law a slot follows.
///
/// Both are integer-only and share the same fixed point; they differ in how the
/// step depends on evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapt {
    /// Fixed geometric step `p += (target - p) >> rate`. Constant vigilance: it
    /// chases a moving source and never fully settles, which is what a
    /// nonstationary context wants.
    Fast,
    /// Count-scaled step `p += (target - p) / (count + 2)`. The step shrinks as
    /// evidence accumulates, so the estimate converges on a fixed source — the
    /// unbiased stationary estimator — at the cost of responding slowly when the
    /// source changes.
    Stationary,
}

/// Why a construction was refused. Every bound is checked here, before any
/// allocation, so a caller cannot request a degenerate or unbounded table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtxMapError {
    /// `bits < MIN_BITS`.
    BitsTooSmall(u32),
    /// `bits > MAX_BITS`.
    BitsTooLarge(u32),
    /// `assoc == 0` or `assoc > MAX_ASSOC`.
    BadAssoc(usize),
    /// The fast-mode shift is outside `MIN_RATE..=MAX_RATE`.
    BadRate(u32),
    /// The requested table would exceed [`MAX_BYTES`] (or overflow `u64`).
    TooLarge,
}

impl core::fmt::Display for CtxMapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CtxMapError::BitsTooSmall(b) => {
                write!(f, "ctxmap: bits {b} below MIN_BITS {MIN_BITS}")
            }
            CtxMapError::BitsTooLarge(b) => {
                write!(f, "ctxmap: bits {b} above MAX_BITS {MAX_BITS}")
            }
            CtxMapError::BadAssoc(a) => {
                write!(f, "ctxmap: associativity {a} outside 1..={MAX_ASSOC}")
            }
            CtxMapError::BadRate(r) => {
                write!(f, "ctxmap: rate {r} outside {MIN_RATE}..={MAX_RATE}")
            }
            CtxMapError::TooLarge => write!(f, "ctxmap: table exceeds MAX_BYTES {MAX_BYTES}"),
        }
    }
}

impl std::error::Error for CtxMapError {}

/// One slot: a probability, its confidence, its run state and its age.
///
/// `tag == 0` is the empty sentinel (`tag_of` never produces 0), so no separate
/// occupancy array is needed. The layout is fixed and asserted at compile time
/// ([`SLOT_BYTES`]) because [`ContextMap::memory_bytes`] must be exact.
#[derive(Debug, Clone, Copy)]
struct Slot {
    /// Checksum of the context that last wrote this slot; 0 means empty.
    tag: u16,
    /// P(1) as `p / 256`, clamped to `1..=255` so a real slot is never 0.
    p: u8,
    /// Observations since the slot was occupied, saturating at [`COUNT_CAP`].
    count: u8,
    /// Length of the current run of identical bits (saturating).
    run: u8,
    /// The last observed bit (0 or 1), for the run state and for callers that
    /// read it (a match/run specialist can consume it directly).
    last: u8,
    /// Monotonic recency stamp; the least-recently-used slot is evicted first.
    stamp: u32,
}

impl Slot {
    const EMPTY: Slot = Slot {
        tag: 0,
        p: DEFAULT_P8,
        count: 0,
        run: 0,
        last: 0,
        stamp: 0,
    };
}

/// Byte size of one slot, taken from the layout rather than hard-coded twice.
/// The compile-time assertion pins the layout so memory accounting cannot drift
/// silently from the representation.
pub const SLOT_BYTES: u64 = core::mem::size_of::<Slot>() as u64;
const _: () = assert!(
    core::mem::size_of::<Slot>() == 12,
    "slot layout changed; revisit SLOT_BYTES and memory_bytes"
);

/// Deterministic 64-bit finaliser. Every set and tag derives from this, so the
/// mapping is a pure function of the key on both encoder and decoder.
#[inline]
fn mix64(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    x
}

/// The 16-bit tag for a mixed key. Zero is reserved for the empty sentinel.
#[inline]
fn tag_of(h: u64) -> u16 {
    let t = (h >> 48) as u16;
    if t == 0 {
        1
    } else {
        t
    }
}

/// The map's 12-bit prediction for an occupied slot: the 8-bit state scaled up.
#[inline]
fn slot_prob(s: &Slot) -> u32 {
    // p in 1..=255 -> 16..=4080, already inside 1..=4095.
    ((s.p as u32) * 16).clamp(1, PROB_SCALE - 1)
}

/// Observed totals, for measurement and for the collision/occupancy report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CtxMapStats {
    /// Slots currently holding a context (`tag != 0`).
    pub occupied: u64,
    /// Total slots (`sets * assoc`).
    pub total: u64,
    /// `update` calls.
    pub updates: u64,
    /// `update` calls whose context already owned a slot (a tag hit).
    pub hits: u64,
    /// `update` calls whose context was absent (a cold start or a real
    /// collision that could not be distinguished from one).
    pub misses: u64,
    /// Insertions that had to displace a live slot (bucket was full).
    pub evictions: u64,
}

/// A set-associative context map.
///
/// Fixed at construction; never grows. `p` is a pure lookup (`&self`), `update`
/// performs insertion, eviction and adaptation. Both are `#[inline]`.
#[derive(Debug, Clone)]
pub struct ContextMap {
    slots: Vec<Slot>,
    sets: usize,
    assoc: usize,
    mask: usize,
    mode: Adapt,
    rate: u32,
    clock: u32,
    stats: CtxMapStats,
}

impl ContextMap {
    /// Build a map with `2^bits` sets and `assoc` ways per set.
    ///
    /// Fails closed on every degenerate or over-large request (see
    /// [`CtxMapError`]): the table's size is a pure function of `bits`, `assoc`
    /// and [`SLOT_BYTES`], so this is also the enforcement of the memory bound.
    pub fn new(bits: u32, assoc: usize, mode: Adapt, rate: u32) -> Result<Self, CtxMapError> {
        if bits < MIN_BITS {
            return Err(CtxMapError::BitsTooSmall(bits));
        }
        if bits > MAX_BITS {
            return Err(CtxMapError::BitsTooLarge(bits));
        }
        if assoc == 0 || assoc > MAX_ASSOC {
            return Err(CtxMapError::BadAssoc(assoc));
        }
        if rate < MIN_RATE || rate > MAX_RATE {
            return Err(CtxMapError::BadRate(rate));
        }
        let sets = 1usize << bits;
        let total = (sets as u64)
            .checked_mul(assoc as u64)
            .ok_or(CtxMapError::TooLarge)?;
        let bytes = total.checked_mul(SLOT_BYTES).ok_or(CtxMapError::TooLarge)?;
        if bytes > MAX_BYTES {
            return Err(CtxMapError::TooLarge);
        }
        Ok(ContextMap {
            slots: vec![Slot::EMPTY; sets * assoc],
            sets,
            assoc,
            mask: sets - 1,
            mode,
            rate,
            clock: 0,
            stats: CtxMapStats {
                total,
                ..CtxMapStats::default()
            },
        })
    }

    /// Number of sets.
    #[inline]
    pub fn sets(&self) -> usize {
        self.sets
    }

    /// Associativity (ways per set).
    #[inline]
    pub fn assoc(&self) -> usize {
        self.assoc
    }

    /// P(1) for `ctx`, in the range coder's 12-bit convention: `1..=4095`.
    ///
    /// A context that owns no slot returns the neutral `PROB_SCALE / 2`. This is
    /// a pure lookup: it never inserts, so repeated calls are identical.
    #[inline]
    pub fn p(&self, ctx: u64) -> u32 {
        let h = mix64(ctx);
        let set = (h as usize) & self.mask;
        let tag = tag_of(h);
        let base = set * self.assoc;
        for i in 0..self.assoc {
            let s = &self.slots[base + i];
            if s.tag == tag {
                return slot_prob(s);
            }
        }
        PROB_SCALE / 2
    }

    /// Observe `bit` for `ctx` (any non-zero `bit` is a 1), inserting the
    /// context or evicting the least-recently-used slot if its set is full.
    #[inline]
    pub fn update(&mut self, ctx: u64, bit: u32) {
        let b = if bit != 0 { 1u8 } else { 0u8 };
        let h = mix64(ctx);
        let set = (h as usize) & self.mask;
        let tag = tag_of(h);
        let base = set * self.assoc;

        let mut found = None;
        for i in 0..self.assoc {
            if self.slots[base + i].tag == tag {
                found = Some(base + i);
                break;
            }
        }
        let idx = match found {
            Some(i) => {
                self.stats.hits += 1;
                i
            }
            None => {
                self.stats.misses += 1;
                // An empty way first; otherwise the stalest (LRU) way. Ties are
                // impossible while the clock is unique, and the scan order makes
                // even a wrapped clock deterministic.
                let mut victim = base;
                let mut empty = None;
                for i in 0..self.assoc {
                    if self.slots[base + i].tag == 0 {
                        empty = Some(base + i);
                        break;
                    }
                }
                if let Some(e) = empty {
                    self.stats.occupied += 1;
                    victim = e;
                } else {
                    self.stats.evictions += 1;
                    let mut best = self.slots[base].stamp;
                    for i in 1..self.assoc {
                        let st = self.slots[base + i].stamp;
                        if st < best {
                            best = st;
                            victim = base + i;
                        }
                    }
                }
                self.slots[victim] = Slot {
                    tag,
                    p: DEFAULT_P8,
                    count: 0,
                    run: 0,
                    last: b,
                    stamp: 0,
                };
                victim
            }
        };

        let slot = &mut self.slots[idx];
        let target: i32 = if b != 0 { 255 } else { 0 };
        let p = slot.p as i32;
        let np = match self.mode {
            Adapt::Fast => p + ((target - p) >> self.rate),
            Adapt::Stationary => p + (target - p) / (slot.count as i32 + 2),
        };
        slot.p = np.clamp(1, 255) as u8;
        if slot.count < COUNT_CAP {
            slot.count += 1;
        }
        if slot.last == b {
            slot.run = slot.run.saturating_add(1);
        } else {
            slot.run = 1;
            slot.last = b;
        }
        self.clock = self.clock.wrapping_add(1);
        slot.stamp = self.clock;
        self.stats.updates += 1;
    }

    /// The last bit observed for `ctx`, if the context currently owns a slot.
    ///
    /// This exposes the run state so a caller building a *match* or run
    /// specialist can fuse it without re-reading the byte history.
    #[inline]
    pub fn last_bit(&self, ctx: u64) -> Option<u32> {
        self.slot_ref(ctx).map(|s| s.last as u32)
    }

    /// The current run length for `ctx`, if the context owns a slot.
    #[inline]
    pub fn run_len(&self, ctx: u64) -> Option<u32> {
        self.slot_ref(ctx).map(|s| s.run as u32)
    }

    #[inline]
    fn slot_ref(&self, ctx: u64) -> Option<&Slot> {
        let h = mix64(ctx);
        let set = (h as usize) & self.mask;
        let tag = tag_of(h);
        let base = set * self.assoc;
        (0..self.assoc)
            .map(|i| &self.slots[base + i])
            .find(|s| s.tag == tag)
    }

    /// Exact table size in bytes: `sets * assoc * SLOT_BYTES`. Constant for the
    /// life of the map; there is no growth path.
    #[inline]
    pub fn memory_bytes(&self) -> u64 {
        self.stats.total * SLOT_BYTES
    }

    /// Observed totals (occupancy, hits, misses, evictions).
    #[inline]
    pub fn stats(&self) -> CtxMapStats {
        self.stats
    }
}

// ---------------------------------------------------------------------------
// Measurement: the context map against the existing direct expert, at equal
// memory, on a corpus slice.
//
// This is the only reason the file exists. Everything below is a diagnostic:
// ideal codelength is `sum(-log2 p(actual bit))` under each model's own
// probabilities, *not* the range coder's output and *not* `S`. Floats appear
// only here, never in the model.
// ---------------------------------------------------------------------------

/// One comparison row: a context-map specialist and a direct expert at the same
/// order and (approximately) the same table memory, over the same bytes.
#[derive(Debug, Clone)]
pub struct BenchRow {
    pub order: usize,
    pub mode: Adapt,
    pub assoc: usize,
    /// Context-map log2 set count.
    pub ctx_bits: u32,
    pub ctx_bytes: u64,
    /// Direct expert log2 slot count.
    pub direct_bits: u32,
    pub direct_bytes: u64,
    /// Ideal codelength in bits (diagnostic only).
    pub ctx_ideal_bits: f64,
    pub direct_ideal_bits: f64,
    /// Collision/occupancy statistics for the context map.
    pub ctx_stats: CtxMapStats,
}

impl BenchRow {
    /// Ideal codelength difference in bytes; negative means the context map is
    /// cheaper on this row. Diagnostic, not authority.
    pub fn delta_bytes(&self) -> f64 {
        (self.ctx_ideal_bits - self.direct_ideal_bits) / 8.0
    }

    /// One aligned, deterministic line.
    pub fn render(&self) -> String {
        format!(
            "order {:>2} {:<10} assoc {} | ctx {:>2}b {:>9}B | direct {:>2}b {:>9}B \
             | mem x{:.2} | ctx {:>10.1}B direct {:>10.1}B delta {:>+10.1}B \
             | occ {:>6}/{:<6} hits {:>9} miss {:>9} evict {:>9}",
            self.order,
            format!("{:?}", self.mode),
            self.assoc,
            self.ctx_bits,
            self.ctx_bytes,
            self.direct_bits,
            self.direct_bytes,
            self.ctx_bytes as f64 / self.direct_bytes.max(1) as f64,
            self.ctx_ideal_bits / 8.0,
            self.direct_ideal_bits / 8.0,
            self.delta_bytes(),
            self.ctx_stats.occupied,
            self.ctx_stats.total,
            self.ctx_stats.hits,
            self.ctx_stats.misses,
            self.ctx_stats.evictions,
        )
    }
}

/// Largest `bits` whose table fits `budget`, or `None` if even [`MIN_BITS`]
/// does not. Bounds are enforced, not asserted.
fn max_bits_for_budget(budget: u64, assoc: u64) -> Option<u32> {
    let unit = SLOT_BYTES.checked_mul(assoc)?;
    let mut b = MAX_BITS;
    loop {
        if (1u64 << b).checked_mul(unit).map_or(false, |v| v <= budget) {
            return Some(b);
        }
        if b == MIN_BITS {
            return None;
        }
        b -= 1;
    }
}

/// The direct expert's log2 slot count whose 2-byte-per-slot table is closest to
/// `ctx_bytes` **in ratio**, so neither model is handed the larger table on a
/// near-tie. Ties prefer the larger direct table: the incumbent should not be
/// the one that is short-changed.
fn closest_direct_bits(ctx_bytes: u64) -> u32 {
    let target_slots = (ctx_bytes / 2).max(1);
    let mut b = 1u32;
    while b < MAX_BITS && (1u64 << (b + 1)) <= target_slots {
        b += 1;
    }
    let bytes_at = |bb: u32| 2u64 << bb;
    let ratio = |m: u64| -> f64 {
        let (a, z) = if m >= ctx_bytes {
            (ctx_bytes.max(1) as f64, m as f64)
        } else {
            (m.max(1) as f64, ctx_bytes.max(1) as f64)
        };
        z / a
    };
    let pick = if ratio(bytes_at(b + 1)) <= ratio(bytes_at(b)) {
        b + 1
    } else {
        b
    };
    pick.clamp(MIN_BITS, MAX_BITS)
}

/// The direct expert's adaptation shift for an order, mirroring the low-order
/// ladder in `ModelConfig::for_size` so the comparison is against the expert the
/// project actually builds.
fn direct_rate(order: usize) -> u32 {
    match order {
        0..=2 => 4,
        3..=6 => 5,
        _ => 6,
    }
}

/// The last `order` bytes of `hist`, hashed with the project's own
/// [`crate::context::hash_bytes`] so the context is identical on encode and
/// decode.
#[inline]
fn order_hash(hist: u64, order: usize) -> u32 {
    let n = order.min(8);
    let mut buf = [0u8; 8];
    for i in 0..n {
        buf[n - 1 - i] = ((hist >> (8 * i)) & 0xFF) as u8;
    }
    crate::context::hash_bytes(&buf[..n])
}

/// Code `data` once with a context-map specialist and once with the project's
/// direct [`crate::context::ContextModel`], both at `order` and at
/// approximately equal memory, and report the ideal codelength of each plus the
/// map's collision/occupancy statistics.
pub fn bench_one(
    data: &[u8],
    order: usize,
    assoc: usize,
    mode: Adapt,
    budget_bytes: u64,
) -> Result<BenchRow, CtxMapError> {
    let ctx_bits = max_bits_for_budget(budget_bytes, assoc as u64).ok_or(CtxMapError::TooLarge)?;
    let ctx_bytes = (1u64 << ctx_bits) * assoc as u64 * SLOT_BYTES;
    let direct_bits = closest_direct_bits(ctx_bytes);

    let mut cmap = ContextMap::new(ctx_bits, assoc, mode, DEFAULT_FAST_RATE)?;
    let mut direct = crate::context::ContextModel::new(direct_bits, direct_rate(order));
    let stretch = crate::mixer::StretchTable::new();

    let mut ctx_ideal = 0.0f64;
    let mut dir_ideal = 0.0f64;
    let mut hist: u64 = 0;

    for &byte in data {
        let ctx_hash = order_hash(hist, order);
        direct.set_context(ctx_hash);
        let mut c0 = 1u32;
        for k in (0..8).rev() {
            let bit = ((byte >> k) & 1) as u32;
            // The map's key is (bytewise context, partial-byte node).
            let key = ((ctx_hash as u64) << 9) | c0 as u64;

            let pc = cmap.p(key) as f64 / PROB_SCALE as f64;
            let qc = if bit != 0 { pc } else { 1.0 - pc };
            ctx_ideal += -qc.log2();
            cmap.update(key, bit);

            let st = direct.predict(c0, &stretch, 32768);
            let pd =
                crate::mixer::squash(st).clamp(1, PROB_SCALE as i32 - 1) as f64 / PROB_SCALE as f64;
            let qd = if bit != 0 { pd } else { 1.0 - pd };
            dir_ideal += -qd.log2();
            direct.update(bit);

            c0 = (c0 << 1) | bit;
        }
        hist = (hist << 8) | byte as u64;
    }

    Ok(BenchRow {
        order,
        mode,
        assoc,
        ctx_bits,
        ctx_bytes,
        direct_bits,
        direct_bytes: (1u64 << direct_bits) * 2,
        ctx_ideal_bits: ctx_ideal,
        direct_ideal_bits: dir_ideal,
        ctx_stats: cmap.stats(),
    })
}

/// The full sweep: orders `{0,1,2,3,4,6}`, associativity `{1,2,4}`, both
/// adaptation modes, at one memory budget, rendered as a deterministic report.
///
/// The report states its own status: ideal codelength is a **diagnostic**, `S`
/// is authority, and the comparison is not tuned. If the context map does not
/// beat the direct expert here, the report says so.
pub fn bench_against_direct(data: &[u8], budget_bytes: u64) -> String {
    const ORDERS: [usize; 6] = [0, 1, 2, 3, 4, 6];
    const ASSOCS: [usize; 3] = [1, 2, 4];
    const MODES: [Adapt; 2] = [Adapt::Fast, Adapt::Stationary];

    let mut out = String::new();
    out.push_str("ctxmap vs direct expert -- ideal codelength (diagnostic, NOT S)\n");
    out.push_str(&format!(
        "corpus slice: {} B | memory budget: {} B per model\n",
        data.len(),
        budget_bytes
    ));
    let mut wins = 0usize;
    let mut rows = 0usize;
    for &order in &ORDERS {
        for &assoc in &ASSOCS {
            for &mode in &MODES {
                match bench_one(data, order, assoc, mode, budget_bytes) {
                    Ok(row) => {
                        if row.ctx_ideal_bits < row.direct_ideal_bits {
                            wins += 1;
                        }
                        rows += 1;
                        out.push_str(&row.render());
                        out.push('\n');
                    }
                    Err(e) => {
                        out.push_str(&format!(
                            "order {order:>2} {mode:?} assoc {assoc}: construction refused: {e}\n"
                        ));
                    }
                }
            }
        }
    }
    out.push_str(&format!(
        "ctxmap cheaper on {wins}/{rows} rows at approximately equal memory.\n"
    ));
    if wins * 2 >= rows {
        out.push_str("verdict: ctxmap is competitive-or-better here on this slice.\n");
    } else {
        out.push_str(
            "verdict: ctxmap LOSES at equal memory on this slice -- the direct expert is\n\
             cheaper on most rows. Reported honestly; the per-slot overhead (12 B vs 2 B)\n\
             buys collision detection and confidence tracking, and on this corpus it does\n\
             not pay for itself.\n",
        );
    }
    out.push_str(
        "ideal codelength is a diagnostic only; S (fully charged submission) is authority.\n\
         No adoption is claimed.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic xorshift64, used only to build test sources.
    fn next(s: &mut u64) -> u64 {
        let mut x = *s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *s = x;
        x
    }

    #[test]
    fn probability_is_always_in_range() {
        for &mode in &[Adapt::Fast, Adapt::Stationary] {
            for &assoc in &[1usize, 2, 4] {
                let mut m = ContextMap::new(6, assoc, mode, 4).unwrap();
                // Cold, before any observation.
                for ctx in 0..64u64 {
                    let p = m.p(ctx);
                    assert!(p >= 1 && p < PROB_SCALE, "cold p={p}");
                }
                for i in 0..5000u64 {
                    let bit = (i % 3 == 0) as u32;
                    let ctx = i.wrapping_mul(2654435761) % 1000;
                    let p = m.p(ctx);
                    assert!(p >= 1 && p < PROB_SCALE, "warm p={p} mode={mode:?}");
                    m.update(ctx, bit);
                    let p = m.p(ctx);
                    assert!(p >= 1 && p < PROB_SCALE, "post p={p}");
                }
            }
        }
    }

    #[test]
    fn memory_is_exactly_the_bound() {
        assert_eq!(SLOT_BYTES, 12);
        for &(bits, assoc) in &[(6u32, 1usize), (8, 2), (10, 4)] {
            let mut m = ContextMap::new(bits, assoc, Adapt::Fast, 4).unwrap();
            let expected = (1u64 << bits) * assoc as u64 * SLOT_BYTES;
            assert_eq!(m.memory_bytes(), expected);
            m.stats(); // stats must not affect memory
            for i in 0..200_000u64 {
                m.update(i.wrapping_mul(2654435761), (i & 1) as u32);
            }
            assert_eq!(m.memory_bytes(), expected, "memory must not grow with use");
        }
    }

    #[test]
    fn degenerate_construction_is_rejected() {
        assert!(ContextMap::new(MIN_BITS - 1, 1, Adapt::Fast, 4).is_err());
        assert!(ContextMap::new(MAX_BITS + 1, 1, Adapt::Fast, 4).is_err());
        assert!(ContextMap::new(8, 0, Adapt::Fast, 4).is_err());
        assert!(ContextMap::new(8, MAX_ASSOC + 1, Adapt::Fast, 4).is_err());
        assert!(ContextMap::new(8, 1, Adapt::Fast, 0).is_err());
        assert!(ContextMap::new(8, 1, Adapt::Fast, MAX_RATE + 1).is_err());
        // Rate is validated for both modes, even though Stationary ignores it.
        assert!(ContextMap::new(8, 1, Adapt::Stationary, 99).is_err());
        // A legal pair of arguments whose product exceeds the byte ceiling.
        assert_eq!(
            ContextMap::new(MAX_BITS, MAX_ASSOC, Adapt::Fast, 4).unwrap_err(),
            CtxMapError::TooLarge
        );
    }

    #[test]
    fn tag_detects_a_collision_instead_of_averaging() {
        const BITS: u32 = 5;
        let mask = (1u64 << BITS) - 1;
        // Find two distinct keys in the same set with different tags.
        let (a, b) = {
            let mut first: Option<(u64, usize, u16)> = None;
            let mut found = None;
            let mut x = 0u64;
            while x < 2_000_000 {
                let h = mix64(x);
                let set = (h as usize) & (mask as usize);
                let tag = tag_of(h);
                match first {
                    None => first = Some((x, set, tag)),
                    Some((_, fs, ft)) if fs == set && ft != tag => {
                        found = Some((first.unwrap().0, x));
                        break;
                    }
                    _ => {}
                }
                x += 1;
            }
            found.expect("two keys sharing a set with distinct tags")
        };

        let mut m = ContextMap::new(BITS, 1, Adapt::Fast, 4).unwrap();
        for _ in 0..300 {
            m.update(a, 1);
        }
        let pa = m.p(a);
        assert!(pa > 3000, "training should make a strongly predict 1: {pa}");
        // b lands in the same slot index but has a different tag: it must be a
        // cold start, not a's learned value.
        assert_eq!(m.p(b), PROB_SCALE / 2);
        // And a's value was not disturbed by the probe.
        assert_eq!(m.p(a), pa);
    }

    #[test]
    fn both_modes_converge_on_a_stationary_source() {
        const CTX: u64 = 0xABCD;
        for &mode in &[Adapt::Fast, Adapt::Stationary] {
            let mut m = ContextMap::new(10, 2, mode, 4).unwrap();
            let mut s = 0x9E37_79B9_7F4A_7C15u64;
            for _ in 0..40_000 {
                let bit = (next(&mut s) % 4 != 0) as u32; // P(1) = 0.75
                m.update(CTX, bit);
            }
            let p = m.p(CTX) as f64 / PROB_SCALE as f64;
            assert!((p - 0.75).abs() < 0.05, "mode {mode:?} settled at {p}");
        }
    }

    #[test]
    fn fast_mode_tracks_a_nonstationary_source_faster() {
        const CTX: u64 = 0x1234;
        let mut fast = ContextMap::new(12, 1, Adapt::Fast, 4).unwrap();
        let mut slow = ContextMap::new(12, 1, Adapt::Stationary, 4).unwrap();
        let mut s = 0x0123_4567_89AB_CDEFu64;
        let mut sum_fast = 0.0f64;
        let mut sum_slow = 0.0f64;
        let mut n = 0u32;
        for i in 0..40_000u32 {
            let high = i < 20_000; // 90% ones, then 10% ones
            let r = next(&mut s) % 10;
            let bit = if high { (r < 9) as u32 } else { (r < 1) as u32 };
            fast.update(CTX, bit);
            slow.update(CTX, bit);
            if (20_000..20_200).contains(&i) {
                sum_fast += fast.p(CTX) as f64;
                sum_slow += slow.p(CTX) as f64;
                n += 1;
            }
        }
        let mf = sum_fast / n as f64;
        let ms = sum_slow / n as f64;
        assert!(
            mf < ms,
            "fast ({mf}) should have moved lower than stationary ({ms}) right after the switch"
        );
    }

    #[test]
    fn determinism_across_runs() {
        fn run(seed: u64) -> (Vec<u32>, CtxMapStats) {
            let mut m = ContextMap::new(7, 2, Adapt::Stationary, 4).unwrap();
            let mut s = seed;
            let mut ps = Vec::new();
            for i in 0..20_000u64 {
                let ctx = next(&mut s) % 4096;
                let bit = (i & 1) as u32;
                ps.push(m.p(ctx));
                m.update(ctx, bit);
            }
            (ps, m.stats())
        }
        let (p1, s1) = run(0xDEAD_BEEF);
        let (p2, s2) = run(0xDEAD_BEEF);
        assert_eq!(p1, p2);
        assert_eq!(s1, s2);
    }

    /// Read the development rung (`evidence/corpus/enwik6`), truncated to `n`
    /// bytes, or fall back to a deterministic synthetic slice so the test is not
    /// brittle on a machine that lacks the corpus.
    fn dev_slice(n: usize) -> (Vec<u8>, String) {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/evidence/corpus/enwik6");
        match std::fs::read(path) {
            Ok(d) => {
                let m = d.len().min(n);
                (d[..m].to_vec(), "evidence/corpus/enwik6".to_string())
            }
            Err(_) => {
                let mut v = Vec::with_capacity(n);
                let mut s = 0x243F_6A88_85A3_08D3u64;
                while v.len() < n {
                    let x = next(&mut s);
                    v.push(b" etaoinshrdlu"[(x % 13) as usize]);
                }
                (v, "synthetic-fallback".to_string())
            }
        }
    }

    #[test]
    fn benchmark_report_reproduces_and_is_honest() {
        let (data, src) = dev_slice(262_144);
        let r1 = bench_against_direct(&data, 1 << 20);
        let r2 = bench_against_direct(&data, 1 << 20);
        assert_eq!(r1, r2, "the report must be deterministic across calls");
        assert!(r1.contains("diagnostic"), "must label ideal codelength");
        assert!(r1.contains("authority"), "must defer to S");
        assert!(r1.contains("ctxmap cheaper on"), "must count wins");
        println!("corpus: {}", src);
        println!("{}", r1);
    }
}
