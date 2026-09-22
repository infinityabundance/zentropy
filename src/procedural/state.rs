//! Phase 14.7: state coding — turning a program's configuration into bytes.
//!
//! A rank is not automatically the best representation, and this module exists to
//! make that a measurement rather than an assumption.
//!
//! For each program `P` there is a finite state model `Θ(P)` — the slots a
//! `Template` may vary, the subsets a `Choice` may take, the counts a `Repeat`
//! may use. The question this module answers, per program, is which of these is
//! smallest *in complete bytes*:
//!
//! ```text
//! raw | varint | delta | rank | block-rank | rANS
//! ```
//!
//! conditioned on the program, its parent, the cohort, the slot, the previous
//! state and the `SignalState` class.
//!
//! Two rules from the plan are load-bearing here:
//!
//! * **A rank is not automatically best.** It wins when the whole mixed-radix
//!   coordinate is cheaper than serializing the dimensions independently, and
//!   loses when the state is skewed — which is what the rANS candidate is for.
//! * **The comparison is on serialized bytes**, never on bits-estimates, and the
//!   model's own description is charged to the stream that uses it. A model that
//!   cannot pay for its own persisted bytes does not exist.
//!
//! ## The state model
//!
//! A [`StateSpace`] is a product of independent dimensions, each with a `u32`
//! radix. A [`State`] is one choice per dimension, held as the canonical digit
//! vector. The four shapes the plan names have constructors:
//!
//! * [`StateSpace::template_slots`] — one dimension per `Template` slot, its radix
//!   the slot's variant count;
//! * [`StateSpace::optional_fields`] — one radix-2 dimension per optional field (a
//!   subset of fields, as a bitmask);
//! * [`StateSpace::permutation`] — the factoradic digits of a permutation, radices
//!   `1, 2, …, n`, so each radix is a `u32` while `n!` itself may exceed `u64`;
//! * [`StateSpace::subset`] — a `k`-subset of `{0..n}` as its colex rank, when that
//!   rank fits a `u32`.
//!
//! ## What this model does **not** claim
//!
//! * **Independence.** The space is a product, so two dimensions whose choices are
//!   correlated are modelled as if independent. That *over*-counts the space, so a
//!   rank is an upper bound on the configuration's information, never a lower one;
//!   the measured competition, not the radix product, decides whether a codec pays.
//! * **Slot contents.** A `Template` slot's *variant count* is a dimension; what a
//!   variant *is* lives in the program (a child node) and is charged there.
//! * **Large permutations.** `n > 20` has no `u64` Lehmer rank and [`super::rank`]
//!   offers none, so [`StateSpace::permutation`] refuses it rather than model a
//!   space it cannot enumerate.
//! * **Large subsets.** A subset whose `C(n, k)` exceeds `u32::MAX` cannot be a
//!   single `u32` radix and [`super::rank`]'s subset rank is `u64`-valued, so
//!   [`StateSpace::subset`] refuses it rather than split it into a coordinate with
//!   no honest inverse.
//! * **Conditional structure.** A count that is a function of another dimension is
//!   not modelled.
//! * **rANS.** Only when the coordinate space fits the coder's 4096-symbol scale;
//!   a larger alphabet would need a table whose description dwarfs the state.

use super::program::Program;
use super::rank::{
    rank_mixed_radix, rank_permutation, rank_subset, unrank_mixed_radix, unrank_permutation,
    unrank_subset,
};
use crate::entropy::rans;

/// Number of codecs. Kept next to [`StateCodec::ALL`] so the two cannot drift.
const CODEC_COUNT: usize = 6;

/// The largest permutation this module represents: [`super::rank::rank_permutation`]
/// refuses `n > 20` because `20!` is the largest factorial that fits a `u64`, and a
/// larger permutation has no `u64` Lehmer rank to expand the factoradic digits from.
pub const MAX_PERMUTATION: u32 = 20;

/// Longest legal LEB128 encoding of a `u64` (`ceil(64/7)`).
const MAX_VARINT_BYTES: usize = 10;

/// The six candidate representations, in the order ties are broken (earliest wins).
///
/// Order matters and is deliberate: on an exact tie the *simpler* codec is chosen,
/// so a caller never pays for a mechanism (a table, a block split) that bought
/// nothing. `Raw` is first because it is the always-available incumbent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateCodec {
    /// Fixed width per dimension: the fewest whole bytes that can hold any value
    /// in the dimension's range, independent of the value.
    Raw,
    /// Each digit as its own unsigned LEB128 varint; exploits small values but not
    /// the product structure of the space.
    Varint,
    /// Each digit's signed difference from the previous state's digit, zigzagged
    /// and varint-coded. Wins on a monotone sequence, where the differences are 0
    /// or 1 regardless of how large the digits are.
    Delta,
    /// The whole mixed-radix coordinate as one varint. Requires the space to fit a
    /// `u64`; a larger space has no single-limb rank.
    Rank,
    /// The same coordinate, split into blocks whose product fits a `u64`, each
    /// block one varint. Representable for *any* radix list, so a space larger than
    /// `u64` is codeable; on a space that fits `u64` it produces the same bytes as
    /// `Rank`.
    BlockRank,
    /// A static rANS stream over the coordinate alphabet, with the histogram
    /// persisted at the front of the stream and charged to it.
    Rans,
}

impl StateCodec {
    /// Every codec, in tie-break order.
    pub const ALL: [StateCodec; CODEC_COUNT] = [
        StateCodec::Raw,
        StateCodec::Varint,
        StateCodec::Delta,
        StateCodec::Rank,
        StateCodec::BlockRank,
        StateCodec::Rans,
    ];

    /// The index into a [`Cost::sizes`] array.
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            StateCodec::Raw => "raw",
            StateCodec::Varint => "varint",
            StateCodec::Delta => "delta",
            StateCodec::Rank => "rank",
            StateCodec::BlockRank => "block-rank",
            StateCodec::Rans => "rans",
        }
    }
}

// ---------------------------------------------------------------------------
// The state model: dimensions, spaces, states.
// ---------------------------------------------------------------------------

/// One dimension's shape. A dimension that is a single radix also reports it via
/// [`Dim::radix`]; [`Dim::Permutation`] expands to several radices and so does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dim {
    /// A bounded digit in `0..radix` — a slot's variant count, a `Repeat` count.
    Count { radix: u32 },
    /// An optional field: present (`1`) or absent (`0`), radix 2.
    Flag,
    /// A `k`-subset of `{0..n}`, held as its colex rank in `0..C(n, k)`. Supported
    /// only when `C(n, k)` fits a `u32`.
    Subset { n: u32, k: u32 },
    /// A permutation of `n` items, expanded into factoradic digits with radices
    /// `1, 2, …, n`. Each radix is a `u32`; the product `n!` may exceed `u64`.
    Permutation { n: u32 },
}

impl Dim {
    /// The number of values this dimension admits, when it is a single radix.
    /// `Permutation` expands rather than being one radix, so it is `None`.
    pub fn radix(self) -> Option<u32> {
        match self {
            Dim::Count { radix } => (radix >= 1).then_some(radix),
            Dim::Flag => Some(2),
            Dim::Subset { n, k } => binomial_u32(n, k).filter(|&r| r >= 1),
            Dim::Permutation { .. } => None,
        }
    }

    /// The radices this dimension expands to, or `None` if it is not representable.
    fn expand(self) -> Option<Vec<u32>> {
        match self {
            Dim::Count { radix } => (radix >= 1).then(|| vec![radix]),
            Dim::Flag => Some(vec![2]),
            Dim::Subset { n, k } => Some(vec![binomial_u32(n, k)?]),
            Dim::Permutation { n } => {
                if n > MAX_PERMUTATION {
                    return None;
                }
                Some((1..=n).collect())
            }
        }
    }
}

/// A finite product space of independent, `u32`-radix dimensions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateSpace {
    dims: Vec<Dim>,
    radices: Vec<u32>,
}

impl StateSpace {
    /// Build from typed dimensions. `None` if any dimension is unrepresentable
    /// (a count radix of zero, a permutation with `n > 20`, or a subset whose
    /// `C(n, k)` exceeds `u32`).
    pub fn from_dims(dims: &[Dim]) -> Option<StateSpace> {
        let mut radices = Vec::new();
        for d in dims {
            radices.extend(d.expand()?);
        }
        Some(StateSpace {
            dims: dims.to_vec(),
            radices,
        })
    }

    /// Build from a bare radix list — the most direct statement of a product space.
    /// A radix of zero is rejected: a zero-width digit is not a coordinate.
    pub fn from_radices(radices: &[u32]) -> Option<StateSpace> {
        if radices.iter().any(|&r| r == 0) {
            return None;
        }
        let dims = radices.iter().map(|&radix| Dim::Count { radix }).collect();
        Some(StateSpace {
            dims,
            radices: radices.to_vec(),
        })
    }

    /// One dimension per `Template` slot, its radix the slot's variant count.
    pub fn template_slots(variant_counts: &[u32]) -> Option<StateSpace> {
        Self::from_radices(variant_counts)
    }

    /// One radix-2 dimension per optional field. `m` is a caller-declared bound,
    /// exactly like a width in [`super::Bounds`]: it is trusted, not read from a
    /// wire.
    pub fn optional_fields(m: u32) -> StateSpace {
        StateSpace {
            dims: vec![Dim::Flag; m as usize],
            radices: vec![2u32; m as usize],
        }
    }

    /// A permutation space: factoradic, so `n!` may exceed `u64` while each radix
    /// stays a `u32`. Refuses `n > 20` (see [`MAX_PERMUTATION`]).
    pub fn permutation(n: u32) -> Option<StateSpace> {
        Self::from_dims(&[Dim::Permutation { n }])
    }

    /// A single `k`-subset-of-`{0..n}` dimension, when `C(n, k)` fits a `u32`.
    pub fn subset(n: u32, k: u32) -> Option<StateSpace> {
        Self::from_dims(&[Dim::Subset { n, k }])
    }

    /// A single bounded count in `0..radix`.
    pub fn bounded_count(radix: u32) -> Option<StateSpace> {
        Self::from_radices(&[radix])
    }

    /// The expanded radices, one per canonical digit.
    pub fn radices(&self) -> &[u32] {
        &self.radices
    }

    /// The typed dimensions (the shapes the caller declared).
    pub fn dims(&self) -> &[Dim] {
        &self.dims
    }

    /// The number of canonical digits.
    pub fn len(&self) -> usize {
        self.radices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.radices.is_empty()
    }

    /// The size of the space, or `None` if it exceeds `u64`.
    pub fn size(&self) -> Option<u64> {
        let mut product: u64 = 1;
        for &r in &self.radices {
            product = product.checked_mul(r as u64)?;
        }
        Some(product)
    }

    /// Whether the whole coordinate fits a single `u64` limb.
    pub fn size_fits_u64(&self) -> bool {
        self.size().is_some()
    }

    /// The coordinate-space size when it fits rANS's table scale, else `None`.
    pub fn rans_alphabet(&self) -> Option<usize> {
        let size = self.size()?;
        let max = 1usize << rans::SCALE_BITS;
        if size >= 1 && size <= max as u64 {
            Some(size as usize)
        } else {
            None
        }
    }

    /// Blocks of consecutive dimensions whose product fits a `u64`. A pure function
    /// of the radices, so encoder and decoder agree on the split without storing it.
    pub fn block_plan(&self) -> Vec<(usize, usize)> {
        let mut plan = Vec::new();
        let mut start = 0usize;
        let mut product: u64 = 1;
        for (i, &r) in self.radices.iter().enumerate() {
            match product.checked_mul(r as u64) {
                Some(p) => product = p,
                None => {
                    plan.push((start, i - start));
                    start = i;
                    product = r as u64;
                }
            }
        }
        if !self.radices.is_empty() {
            plan.push((start, self.radices.len() - start));
        }
        plan
    }

    /// Whether `state` is a legal coordinate of this space: right length, every
    /// digit inside its radix.
    pub fn validate(&self, state: &State) -> bool {
        state.digits.len() == self.radices.len()
            && state.digits.iter().zip(&self.radices).all(|(&d, &r)| d < r)
    }

    /// Validate and wrap a digit vector. The checked constructor: an out-of-range
    /// vector is `None`, never an aliased state.
    pub fn state(&self, digits: &[u32]) -> Option<State> {
        let st = State {
            digits: digits.to_vec(),
        };
        if self.validate(&st) {
            Some(st)
        } else {
            None
        }
    }

    /// The all-zero state (the empty state of a product space).
    pub fn zeros(&self) -> State {
        State {
            digits: vec![0u32; self.radices.len()],
        }
    }

    /// The largest state the space admits: every digit at `radix - 1`.
    pub fn max(&self) -> State {
        State {
            digits: self.radices.iter().map(|&r| r - 1).collect(),
        }
    }

    /// The mixed-radix coordinate of `state`, or `None` if it is out of range or
    /// does not fit a `u64`.
    pub fn coordinate(&self, state: &State) -> Option<u64> {
        if !self.validate(state) {
            return None;
        }
        let rank = rank_mixed_radix(&self.radices, &state.digits)?;
        if rank.len() != 1 {
            return None;
        }
        Some(rank[0])
    }

    /// The state at coordinate `coord`, or `None` if `coord` is outside the space.
    pub fn from_coordinate(&self, coord: u64) -> Option<State> {
        let digits = unrank_mixed_radix(&self.radices, &[coord])?;
        Some(State { digits })
    }

    /// The canonical state of `perm`, for a permutation space.
    ///
    /// Uses [`rank_permutation`] and the factoradic expansion, so the digits are
    /// the Lehmer code in reverse place order — the representation the `Rank`
    /// codec actually charges for.
    pub fn permutation_state(&self, perm: &[u32]) -> Option<State> {
        let n = match self.dims.as_slice() {
            [Dim::Permutation { n }] => *n,
            _ => return None,
        };
        if perm.len() as u32 != n {
            return None;
        }
        let rank = rank_permutation(perm)?;
        let digits = unrank_mixed_radix(&self.radices, &[rank])?;
        Some(State { digits })
    }

    /// The permutation a state names, for a permutation space, up to `n = 20`.
    pub fn as_permutation(&self, state: &State) -> Option<Vec<u32>> {
        let n = match self.dims.as_slice() {
            [Dim::Permutation { n }] => *n,
            _ => return None,
        };
        if !self.validate(state) {
            return None;
        }
        let rank = rank_mixed_radix(&self.radices, &state.digits)?;
        if rank.len() != 1 {
            return None;
        }
        unrank_permutation(n, rank[0])
    }

    /// The canonical state of the `k`-subset `set`, for a subset space.
    pub fn subset_state(&self, set: &[u32]) -> Option<State> {
        let (n, k) = match self.dims.as_slice() {
            [Dim::Subset { n, k }] => (*n, *k),
            _ => return None,
        };
        let rank = rank_subset(n, k, set)?;
        if rank > u32::MAX as u64 {
            return None;
        }
        Some(State {
            digits: vec![rank as u32],
        })
    }

    /// The subset a state names, for a subset space.
    pub fn as_subset(&self, state: &State) -> Option<Vec<u32>> {
        let (n, k) = match self.dims.as_slice() {
            [Dim::Subset { n, k }] => (*n, *k),
            _ => return None,
        };
        if state.digits.len() != 1 {
            return None;
        }
        unrank_subset(n, k, state.digits[0] as u64)
    }

    /// The state of an optional-field bitmask, for an optional-fields space.
    pub fn flags_state(&self, mask: u64) -> Option<State> {
        if self.dims.iter().any(|d| *d != Dim::Flag) {
            return None;
        }
        let m = self.dims.len();
        if m > 64 || (m < 64 && (mask >> m) != 0) {
            return None;
        }
        Some(State {
            digits: (0..m).map(|i| ((mask >> i) & 1) as u32).collect(),
        })
    }

    /// The bitmask a state names, for an optional-fields space of at most 64 fields.
    pub fn flags(&self, state: &State) -> Option<u64> {
        if self.dims.iter().any(|d| *d != Dim::Flag) {
            return None;
        }
        if state.digits.len() != self.dims.len() || state.digits.len() > 64 {
            return None;
        }
        let mut mask = 0u64;
        for (i, &d) in state.digits.iter().enumerate() {
            if d >= 2 {
                return None;
            }
            mask |= (d as u64) << i;
        }
        Some(mask)
    }
}

/// A concrete configuration: one digit per dimension, in canonical order.
///
/// `State` is deliberately small and copyable-by-value; validity is a property of
/// a `(StateSpace, State)` pair, checked by [`StateSpace::validate`], so a state
/// that does not belong to a space is a `None` at the codec boundary rather than a
/// sentinel inside the type.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    digits: Vec<u32>,
}

impl State {
    /// Wrap a digit vector *without* validation. Prefer [`StateSpace::state`] when
    /// the space is known: an unchecked state is rejected at encode time.
    pub fn from_digits(digits: Vec<u32>) -> State {
        State { digits }
    }

    /// The canonical digits.
    pub fn digits(&self) -> &[u32] {
        &self.digits
    }

    pub fn is_empty(&self) -> bool {
        self.digits.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Conditioning.
// ---------------------------------------------------------------------------

/// The conditioning context the plan lists (program, parent, cohort, slot,
/// previous state, `SignalState` class).
///
/// Every field is *causal*: the decoder can reproduce it from bytes already coded
/// (or from state the archive persisted), so a codec may read any of it without
/// breaking reconstruction.
///
/// **Actually used by the codecs in this module:**
///
/// * `previous` — the predecessor in coding order, the base for [`StateCodec::Delta`];
/// * `cohort` — the empirical distribution [`StateCodec::Rans`] builds its static
///   histogram from (and, when present, the default predecessor for a stream).
///
/// **Reserved** (carried so the interface does not change when a model that uses
/// them lands): `program`, `parent`, `slot`, `signal_class`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Conditioning<'a> {
    /// The program whose state this is. Reserved.
    pub program: Option<&'a Program>,
    /// The parent program, for shared or inherited state. Reserved.
    pub parent: Option<&'a Program>,
    /// The observed cohort a static model may be learned from. **Used by `Rans`.**
    pub cohort: Option<&'a [State]>,
    /// The slot index within a `Template`. Reserved.
    pub slot: Option<u32>,
    /// The previous state in coding order. **Used by `Delta`.**
    pub previous: Option<&'a State>,
    /// The coarse structural class of the current `SignalState` (for example
    /// `crate::signal::ZirClass::code()`). Reserved.
    pub signal_class: Option<u32>,
}

// ---------------------------------------------------------------------------
// Primitive coders.
// ---------------------------------------------------------------------------

fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let low = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// Read one unsigned LEB128 value, or `None` for truncation, an over-long run, or
/// a value whose set bits do not fit a `u64`. Rejecting rather than wrapping is
/// what stops a forged length from aliasing a small one.
fn get_uvarint(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..MAX_VARINT_BYTES {
        let byte = *b.get(*pos)?;
        *pos += 1;
        if shift >= 64 {
            return None;
        }
        let low = (byte & 0x7f) as u64;
        if shift == 63 && low > 1 {
            return None;
        }
        v |= low << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
    None
}

/// Signed-to-unsigned bijection used by `Delta`; the inverse is [`unzigzag`].
#[inline]
fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

#[inline]
fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// Whole bytes needed to hold any value in `0..radix` at fixed width. A radix of
/// one needs none: there is no choice to record.
fn bytes_for(radix: u32) -> usize {
    let mut n = 0usize;
    let mut cap: u64 = 1;
    while cap < radix as u64 {
        cap = cap.saturating_mul(256);
        n += 1;
    }
    n
}

/// `C(n, k)` when it fits a `u32` and `k <= n`, else `None`. Exact integer
/// arithmetic; the early return is safe because `C(n, i)` is monotone for
/// `i <= n/2` and `k` is folded to the smaller half.
fn binomial_u32(n: u32, k: u32) -> Option<u32> {
    if k > n {
        return None;
    }
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 0..k as u64 {
        r = r.checked_mul((n as u64 - i) as u128)?;
        r /= (i + 1) as u128;
        if r > u32::MAX as u128 {
            return None;
        }
    }
    u32::try_from(r).ok()
}

/// The predecessor digits a `Delta` coding is measured against: the previous
/// state, or all zeros when there is none. A predecessor of the wrong shape is
/// rejected rather than silently zero-filled.
fn prior_digits(space: &StateSpace, ctx: &Conditioning) -> Option<Vec<u32>> {
    match ctx.previous {
        None => Some(vec![0u32; space.radices.len()]),
        Some(p) if space.validate(p) => Some(p.digits.clone()),
        Some(_) => None,
    }
}

// ---------------------------------------------------------------------------
// The rANS model. The histogram is the stream's own description: it is written in
// front of the coded symbols and charged to them, because a model that cannot pay
// for its own persisted bytes does not exist.
// ---------------------------------------------------------------------------

/// Whether `entropy::rans::Table::from_counts` can normalise these counts without
/// tripping its own assertion. That function is the single source of truth, but it
/// *panics* on a pathological vector, and a forged archive must produce `None`, not
/// a panic — so the dangerous adjustment is mirrored here and checked first.
fn rans_counts_ok(counts: &[u32]) -> bool {
    let m: u64 = 1u64 << rans::SCALE_BITS;
    if counts.is_empty() || counts.len() as u64 > m {
        return false;
    }
    let total: u64 = counts.iter().map(|&c| c as u64).sum();
    if total == 0 {
        return true; // the degenerate uniform branch, which has no assertion
    }
    let mut assigned: u64 = 0;
    let mut max_freq: u64 = 0;
    for &c in counts {
        if c == 0 {
            continue;
        }
        let f = ((c as u64 * m) / total).max(1);
        assigned += f;
        if f > max_freq {
            max_freq = f;
        }
    }
    if assigned == m {
        return true;
    }
    // `from_counts` adjusts the largest entry by `m - assigned`; it must stay >= 1.
    max_freq as i64 + (m as i64 - assigned as i64) >= 1
}

/// Build a `Table` from counts that have passed [`rans_counts_ok`].
fn rans_table(counts: &[u32]) -> Option<rans::Table> {
    if !rans_counts_ok(counts) {
        return None;
    }
    Some(rans::Table::from_counts(counts))
}

/// Persist a histogram and code `coords` with it. Returns the bytes and the number
/// of them that are the model, so a caller can charge the split honestly.
fn rans_encode_coords(alphabet: usize, coords: &[u32]) -> Option<(Vec<u8>, usize)> {
    if alphabet == 0 || alphabet > (1usize << rans::SCALE_BITS) {
        return None;
    }
    let mut counts = vec![0u32; alphabet];
    for &c in coords {
        let idx = c as usize;
        if idx >= alphabet {
            return None;
        }
        counts[idx] = counts[idx].checked_add(1)?;
    }
    let table = rans_table(&counts)?;
    let mut out = Vec::new();
    for &c in &counts {
        put_uvarint(&mut out, c as u64);
    }
    let model_len = out.len();
    let symbols: Vec<u16> = coords.iter().map(|&c| c as u16).collect();
    out.extend_from_slice(&rans::encode(&symbols, &table));
    Some((out, model_len))
}

/// Read a persisted histogram, advancing `pos` past it.
fn rans_decode_table(bytes: &[u8], pos: &mut usize, alphabet: usize) -> Option<rans::Table> {
    if alphabet == 0 || alphabet > (1usize << rans::SCALE_BITS) {
        return None;
    }
    let mut counts = Vec::with_capacity(alphabet);
    for _ in 0..alphabet {
        let c = get_uvarint(bytes, pos)?;
        if c > u32::MAX as u64 {
            return None;
        }
        counts.push(c as u32);
    }
    rans_table(&counts)
}

/// Encode a whole stream of states with one persisted histogram: the cohort's
/// distribution plus the states themselves, so every coded symbol has positive
/// frequency and the model always covers what it codes.
fn rans_encode_stream(
    space: &StateSpace,
    states: &[State],
    ctx: &Conditioning,
) -> Option<(Vec<u8>, usize)> {
    let alphabet = space.rans_alphabet()?;
    let mut coords: Vec<u32> = Vec::new();
    if let Some(cohort) = ctx.cohort {
        for s in cohort {
            if let Some(c) = space.coordinate(s) {
                coords.push(c as u32);
            }
        }
    }
    for s in states {
        coords.push(space.coordinate(s)? as u32);
    }
    rans_encode_coords(alphabet, &coords)
}

// ---------------------------------------------------------------------------
// Encode / decode.
// ---------------------------------------------------------------------------

impl StateCodec {
    /// Encode one state against `space`, or `None` if the state is not in the
    /// space.
    ///
    /// The plan sketches `encode_state(..) -> Vec<u8>`; this returns `Option`
    /// instead, because a rank has no representation for a coordinate outside its
    /// space and the doctrine inherited from [`super::rank`] is that an illegal
    /// coordinate is `None` — never a panic and never a sentinel that would alias a
    /// different state. `decode_state` is the untrusted inverse and is *also*
    /// `Option`.
    pub fn encode_state(
        self,
        space: &StateSpace,
        state: &State,
        ctx: &Conditioning,
    ) -> Option<Vec<u8>> {
        if !space.validate(state) {
            return None;
        }
        match self {
            StateCodec::Raw => {
                let mut out = Vec::new();
                for (&d, &r) in state.digits.iter().zip(&space.radices) {
                    let w = bytes_for(r);
                    for b in 0..w {
                        out.push((d >> (8 * b)) as u8);
                    }
                }
                Some(out)
            }
            StateCodec::Varint => {
                let mut out = Vec::new();
                for &d in &state.digits {
                    put_uvarint(&mut out, d as u64);
                }
                Some(out)
            }
            StateCodec::Delta => {
                let prior = prior_digits(space, ctx)?;
                let mut out = Vec::new();
                for (&d, &p) in state.digits.iter().zip(&prior) {
                    put_uvarint(&mut out, zigzag(d as i64 - p as i64));
                }
                Some(out)
            }
            StateCodec::Rank => {
                let rank = rank_mixed_radix(&space.radices, &state.digits)?;
                if rank.len() != 1 {
                    return None; // space exceeds u64: no single-limb rank
                }
                let mut out = Vec::new();
                put_uvarint(&mut out, rank[0]);
                Some(out)
            }
            StateCodec::BlockRank => {
                let mut out = Vec::new();
                for (start, len) in space.block_plan() {
                    let rank = rank_mixed_radix(
                        &space.radices[start..start + len],
                        &state.digits[start..start + len],
                    )?;
                    if rank.len() != 1 {
                        return None;
                    }
                    put_uvarint(&mut out, rank[0]);
                }
                Some(out)
            }
            StateCodec::Rans => {
                // Self-contained: persist the model, then code the one symbol. The
                // cohort contributes its distribution and the state contributes
                // itself, so the coded symbol always has positive frequency.
                let alphabet = space.rans_alphabet()?;
                let mut coords: Vec<u32> = Vec::new();
                if let Some(cohort) = ctx.cohort {
                    for s in cohort {
                        if let Some(c) = space.coordinate(s) {
                            coords.push(c as u32);
                        }
                    }
                }
                coords.push(space.coordinate(state)? as u32);
                rans_encode_coords(alphabet, &coords).map(|(b, _)| b)
            }
        }
    }

    /// Decode one state, or `None` if the bytes are not a canonical encoding of a
    /// state in `space`. Never panics: an out-of-range digit, an over-long rank, a
    /// truncated varint and trailing bytes are all typed rejections.
    pub fn decode_state(
        self,
        space: &StateSpace,
        bytes: &[u8],
        ctx: &Conditioning,
    ) -> Option<State> {
        let mut pos = 0usize;
        let st = self.decode_at(space, bytes, &mut pos, ctx)?;
        if pos != bytes.len() {
            return None;
        }
        Some(st)
    }

    /// Decode one state from `bytes` at `*pos`, advancing `*pos`. The tail of a
    /// rANS stream is its own payload, so `Rans` consumes the rest of the slice.
    fn decode_at(
        self,
        space: &StateSpace,
        bytes: &[u8],
        pos: &mut usize,
        ctx: &Conditioning,
    ) -> Option<State> {
        match self {
            StateCodec::Raw => {
                let mut digits = Vec::with_capacity(space.radices.len());
                for &r in &space.radices {
                    let w = bytes_for(r);
                    let mut v: u32 = 0;
                    for b in 0..w {
                        let byte = *bytes.get(*pos)?;
                        *pos += 1;
                        v |= (byte as u32) << (8 * b);
                    }
                    if v >= r {
                        return None;
                    }
                    digits.push(v);
                }
                Some(State { digits })
            }
            StateCodec::Varint => {
                let mut digits = Vec::with_capacity(space.radices.len());
                for &r in &space.radices {
                    let v = get_uvarint(bytes, pos)?;
                    if v >= r as u64 {
                        return None;
                    }
                    digits.push(v as u32);
                }
                Some(State { digits })
            }
            StateCodec::Delta => {
                let prior = prior_digits(space, ctx)?;
                let mut digits = Vec::with_capacity(prior.len());
                for &p in &prior {
                    let z = get_uvarint(bytes, pos)?;
                    let d = (p as i64).checked_add(unzigzag(z))?;
                    if d < 0 || d > u32::MAX as i64 {
                        return None;
                    }
                    digits.push(d as u32);
                }
                let st = State { digits };
                if !space.validate(&st) {
                    return None;
                }
                Some(st)
            }
            StateCodec::Rank => {
                let v = get_uvarint(bytes, pos)?;
                let digits = unrank_mixed_radix(&space.radices, &[v])?;
                Some(State { digits })
            }
            StateCodec::BlockRank => {
                let mut digits = Vec::with_capacity(space.radices.len());
                for (start, len) in space.block_plan() {
                    let v = get_uvarint(bytes, pos)?;
                    let block = unrank_mixed_radix(&space.radices[start..start + len], &[v])?;
                    digits.extend(block);
                }
                Some(State { digits })
            }
            StateCodec::Rans => {
                let alphabet = space.rans_alphabet()?;
                let table = rans_decode_table(bytes, pos, alphabet)?;
                let mut dec = rans::Decoder::new(&bytes[*pos..])?;
                let symbol = dec.decode(&table)? as u64;
                *pos = bytes.len();
                space.from_coordinate(symbol)
            }
        }
    }

    /// Encode a stream of states. For every codec except `Rans` this is the
    /// concatenation of the per-state encodings (each is self-delimiting given the
    /// space); `Rans` codes the whole stream against one persisted histogram.
    pub fn encode_stream(
        self,
        space: &StateSpace,
        states: &[State],
        ctx: &Conditioning,
    ) -> Option<Vec<u8>> {
        self.encode_complete(space, states, ctx).map(|(b, _)| b)
    }

    /// Like [`StateCodec::encode_stream`], but also reports how many of the bytes
    /// are the rANS model, so the model can be charged where it is spent.
    fn encode_complete(
        self,
        space: &StateSpace,
        states: &[State],
        ctx: &Conditioning,
    ) -> Option<(Vec<u8>, usize)> {
        if states.is_empty() {
            return Some((Vec::new(), 0));
        }
        for s in states {
            if !space.validate(s) {
                return None;
            }
        }
        if self == StateCodec::Rans {
            return rans_encode_stream(space, states, ctx);
        }
        let mut out = Vec::new();
        let mut prev: Option<State> = ctx.previous.cloned();
        for s in states {
            let mut local = *ctx;
            local.previous = prev.as_ref();
            out.extend_from_slice(&self.encode_state(space, s, &local)?);
            prev = Some(s.clone());
        }
        Some((out, 0))
    }

    /// Decode exactly `n` states from `bytes`, the inverse of
    /// [`StateCodec::encode_stream`].
    pub fn decode_stream(
        self,
        space: &StateSpace,
        bytes: &[u8],
        n: usize,
        ctx: &Conditioning,
    ) -> Option<Vec<State>> {
        if n == 0 {
            return if bytes.is_empty() {
                Some(Vec::new())
            } else {
                None
            };
        }
        if self == StateCodec::Rans {
            let alphabet = space.rans_alphabet()?;
            let mut pos = 0usize;
            let table = rans_decode_table(bytes, &mut pos, alphabet)?;
            let mut dec = rans::Decoder::new(&bytes[pos..])?;
            let mut symbols = Vec::new();
            if !dec.decode_n(&table, n, &mut symbols) {
                return None;
            }
            return symbols
                .into_iter()
                .map(|s| space.from_coordinate(s as u64))
                .collect();
        }
        let mut out = Vec::with_capacity(n);
        let mut pos = 0usize;
        let mut prev: Option<State> = ctx.previous.cloned();
        for _ in 0..n {
            let mut local = *ctx;
            local.previous = prev.as_ref();
            let st = self.decode_at(space, bytes, &mut pos, &local)?;
            prev = Some(st.clone());
            out.push(st);
        }
        if pos != bytes.len() {
            return None;
        }
        Some(out)
    }
}

// ---------------------------------------------------------------------------
// The measured competition.
// ---------------------------------------------------------------------------

/// The complete sizes each codec achieved, so a caller can see *why* a codec won.
///
/// `sizes[codec.index()].1` is `usize::MAX` for a codec the space cannot represent
/// (`Rank` above `u64`, `Rans` above its 4096-symbol scale), which is distinct from
/// a codec that was representable but large.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cost {
    pub sizes: [(StateCodec, usize); CODEC_COUNT],
    /// Bytes of the rANS stream that are its persisted histogram.
    pub rans_model_bytes: usize,
    /// Bytes of the rANS stream that are coded symbols.
    pub rans_stream_bytes: usize,
}

impl Cost {
    /// The complete size a codec achieved, or `usize::MAX` if it cannot represent
    /// the space.
    pub fn size(&self, codec: StateCodec) -> usize {
        self.sizes[codec.index()].1
    }
}

/// The winner of the competition, with its bytes and the size breakdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub codec: StateCodec,
    pub bytes: Vec<u8>,
    pub cost: Cost,
}

impl Choice {
    /// The winning complete size, in bytes.
    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    /// A one-line receipt explaining the win. Integer-only: no floats reach a
    /// receipt.
    pub fn explain(&self) -> String {
        let mut s = format!("{} wins at {} B", self.codec.name(), self.bytes.len());
        for &(codec, size) in &self.cost.sizes {
            if codec == self.codec {
                continue;
            }
            if size == usize::MAX {
                s.push_str(&format!("; {} n/a", codec.name()));
            } else {
                s.push_str(&format!("; {} {} B", codec.name(), size));
            }
        }
        if self.cost.rans_model_bytes > 0 {
            s.push_str(&format!(
                " (rans model {} B + symbols {} B)",
                self.cost.rans_model_bytes, self.cost.rans_stream_bytes
            ));
        }
        s
    }
}

/// Compare every codec on a program's state stream and return the smallest
/// *complete* serialized result.
///
/// This is the plan's `best_codec(..) -> (StateCodec, Vec<u8>)` plus the byte
/// breakdown the caller needs to see why it won: [`Choice::codec`] and
/// [`Choice::bytes`] are that pair, and [`Choice::cost`] is the reason. The rANS
/// histogram is charged inside its own size, so a model only exists if it pays for
/// itself.
///
/// `states` is the program's state stream; a caller with a single state passes a
/// one-element slice. `states` must be valid in `space`.
pub fn best_codec(space: &StateSpace, states: &[State], ctx: &Conditioning) -> Choice {
    // Seed each slot with its *own* codec name, so an unrepresentable codec (whose
    // size stays `usize::MAX`) is still labelled correctly by `explain` rather than
    // inheriting `Raw`'s name from a default fill.
    let mut sizes = StateCodec::ALL.map(|c| (c, usize::MAX));
    let mut best = StateCodec::Raw;
    let mut best_bytes: Vec<u8> = Vec::new();
    let mut have_best = false;
    let mut rans_model_bytes = 0usize;
    let mut rans_stream_bytes = 0usize;
    for codec in StateCodec::ALL {
        if let Some((bytes, model)) = codec.encode_complete(space, states, ctx) {
            sizes[codec.index()] = (codec, bytes.len());
            if codec == StateCodec::Rans {
                rans_model_bytes = model;
                rans_stream_bytes = bytes.len() - model;
            }
            // Strict `<` keeps the earliest codec on a tie, which is the simpler
            // mechanism; `all_codes` order is the tie-break priority.
            if !have_best || bytes.len() < best_bytes.len() {
                best = codec;
                best_bytes = bytes;
                have_best = true;
            }
        }
    }
    Choice {
        codec: best,
        bytes: best_bytes,
        cost: Cost {
            sizes,
            rans_model_bytes,
            rans_stream_bytes,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests. Each is a claim about behaviour, and each is built so it *can* fail.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic xorshift so the tests are reproducible without a dependency.
    fn xorshift(state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    fn random_state(space: &StateSpace, seed: &mut u64) -> State {
        let digits: Vec<u32> = space
            .radices()
            .iter()
            .map(|&r| (xorshift(seed) % r as u64) as u32)
            .collect();
        space
            .state(&digits)
            .expect("digits are in range by construction")
    }

    #[test]
    fn every_codec_round_trips_including_the_empty_and_maximal_states() {
        let spaces = [
            StateSpace::from_radices(&[]).unwrap(),
            StateSpace::from_radices(&[1]).unwrap(),
            StateSpace::from_radices(&[2, 3, 5]).unwrap(),
            StateSpace::from_radices(&[1 << 20, 7]).unwrap(),
            StateSpace::permutation(5).unwrap(),
            StateSpace::subset(6, 3).unwrap(),
            StateSpace::optional_fields(5),
            StateSpace::from_radices(&[1 << 31, 1 << 31, 1 << 31]).unwrap(),
        ];
        let ctx = Conditioning::default();
        for space in &spaces {
            // The empty state, the largest state the space admits, and a few
            // interior states.
            let mut states = vec![space.zeros(), space.max()];
            let mut seed = 0x1234_5678_9abc_def0u64;
            for _ in 0..4 {
                states.push(random_state(space, &mut seed));
            }
            for &codec in &StateCodec::ALL {
                for st in &states {
                    match codec.encode_state(space, st, &ctx) {
                        Some(bytes) => {
                            let back =
                                codec.decode_state(space, &bytes, &ctx).unwrap_or_else(|| {
                                    panic!("{codec:?} failed to decode {st:?} in {space:?}")
                                });
                            assert_eq!(&back, st, "{codec:?} in {space:?}");
                        }
                        None => match codec {
                            // Only these two may be unrepresentable, and only for
                            // the reasons documented on the codec.
                            StateCodec::Rank => assert!(!space.size_fits_u64()),
                            StateCodec::Rans => assert!(space.rans_alphabet().is_none()),
                            other => panic!("{other:?} unexpectedly unrepresentable"),
                        },
                    }
                }
            }
        }
    }

    #[test]
    fn rank_beats_raw_on_a_small_dense_state_space() {
        // 4^3 = 64 states: each rank is under 128, so it is one byte, while raw
        // spends one byte per dimension.
        let space = StateSpace::from_radices(&[4, 4, 4]).unwrap();
        let mut states = Vec::new();
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    states.push(space.state(&[a, b, c]).unwrap());
                }
            }
        }
        let ctx = Conditioning::default();
        let raw = StateCodec::Raw
            .encode_stream(&space, &states, &ctx)
            .unwrap();
        let rank = StateCodec::Rank
            .encode_stream(&space, &states, &ctx)
            .unwrap();
        assert!(
            rank.len() < raw.len(),
            "rank {} B is not smaller than raw {} B",
            rank.len(),
            raw.len()
        );
        // And the winner must be the measured one, not a guess.
        let choice = best_codec(&space, &states, &ctx);
        assert_eq!(choice.codec, StateCodec::Rank);
        assert_eq!(choice.bytes, rank);
    }

    #[test]
    fn rans_beats_rank_on_a_skewed_state_stream() {
        // 90/5/3/2 over a 256-symbol alphabet: under Rank every state costs its
        // varint, under rANS the common symbol costs a fraction of a bit.
        let space = StateSpace::from_radices(&[256]).unwrap();
        let mut states = Vec::new();
        let mut seed = 0xdead_beef_0bad_f00du64;
        for _ in 0..20_000 {
            let r = xorshift(&mut seed) % 100;
            let sym = if r < 90 {
                0
            } else if r < 95 {
                1
            } else if r < 98 {
                2
            } else {
                3
            };
            states.push(space.state(&[sym]).unwrap());
        }
        let ctx = Conditioning::default();
        let rank = StateCodec::Rank
            .encode_stream(&space, &states, &ctx)
            .unwrap();
        let (rans, model) = rans_encode_stream(&space, &states, &ctx).unwrap();
        assert!(
            rans.len() < rank.len(),
            "rans {} B (model {}) is not smaller than rank {} B",
            rans.len(),
            model,
            rank.len()
        );
        let choice = best_codec(&space, &states, &ctx);
        assert_eq!(choice.codec, StateCodec::Rans);
    }

    #[test]
    fn rans_loses_to_rank_on_a_uniform_large_alphabet_once_the_model_is_charged() {
        // A near-uniform 4096-symbol alphabet. The coded symbols alone are cheaper
        // than Rank's varints, but the 4096 persisted counts cost more than the
        // difference — which is exactly why the model must be charged.
        let space = StateSpace::from_radices(&[4096]).unwrap();
        let states: Vec<State> = (0..4096u32).map(|i| space.state(&[i]).unwrap()).collect();
        let ctx = Conditioning::default();
        let choice = best_codec(&space, &states, &ctx);

        let cost = &choice.cost;
        let raw = cost.size(StateCodec::Raw);
        let rank = cost.size(StateCodec::Rank);
        let rans = cost.size(StateCodec::Rans);

        assert!(
            cost.rans_model_bytes >= 4096,
            "model not charged: {:?}",
            cost
        );
        assert!(
            cost.rans_stream_bytes < raw,
            "the coded rANS symbols ({}) should be smaller than raw ({})",
            cost.rans_stream_bytes,
            raw
        );
        assert!(
            rans > rank,
            "rans total {} B must lose to rank {} B once the model is charged",
            rans,
            rank
        );
        // If the model were free, rANS would have won; the winner says it did not.
        assert_ne!(choice.codec, StateCodec::Rans);
        assert!(choice.size() <= raw);
    }

    #[test]
    fn block_rank_round_trips_a_state_space_larger_than_u64() {
        // 2^31 * 2^31 * 2^31 = 2^93, well past u64.
        let space = StateSpace::from_radices(&[1 << 31, 1 << 31, 1 << 31]).unwrap();
        assert!(!space.size_fits_u64());
        assert_eq!(space.size(), None);
        assert!(space.block_plan().len() >= 2, "need more than one block");

        let ctx = Conditioning::default();
        let samples = [
            space.zeros(),
            space.max(),
            space.state(&[1, 2, 3]).unwrap(),
            space.state(&[0x7fff_ffff, 0, 1]).unwrap(),
        ];
        for st in &samples {
            let bytes = StateCodec::BlockRank
                .encode_state(&space, st, &ctx)
                .unwrap();
            let back = StateCodec::BlockRank
                .decode_state(&space, &bytes, &ctx)
                .unwrap();
            assert_eq!(&back, st);
        }
        // Rank represents an individual coordinate that happens to fit a u64 (the
        // zero state is rank 0), but the largest state in a space this size has no
        // single-limb rank — which is exactly why BlockRank exists.
        assert_eq!(
            StateCodec::Rank.encode_state(&space, &space.zeros(), &ctx),
            Some(vec![0])
        );
        assert_eq!(
            StateCodec::Rank.encode_state(&space, &space.max(), &ctx),
            None
        );
        let max_bytes = StateCodec::BlockRank
            .encode_state(&space, &space.max(), &ctx)
            .unwrap();
        assert!(max_bytes.len() >= 2);
        // And the stream API agrees with the per-state API.
        let stream = StateCodec::BlockRank
            .encode_stream(&space, &samples, &ctx)
            .unwrap();
        let back = StateCodec::BlockRank
            .decode_stream(&space, &stream, samples.len(), &ctx)
            .unwrap();
        assert_eq!(back, samples.to_vec());
    }

    #[test]
    fn delta_beats_raw_for_a_monotone_sequence() {
        // A 20-bit dimension with a fixed raw width of three bytes, advanced by one
        // each step: every delta is 0 or 1, so every delta is one byte.
        let space = StateSpace::from_radices(&[1 << 20]).unwrap();
        let states: Vec<State> = (0..1000u32).map(|i| space.state(&[i]).unwrap()).collect();
        let ctx = Conditioning::default();
        let raw = StateCodec::Raw
            .encode_stream(&space, &states, &ctx)
            .unwrap();
        let delta = StateCodec::Delta
            .encode_stream(&space, &states, &ctx)
            .unwrap();
        assert!(
            delta.len() < raw.len(),
            "delta {} B is not smaller than raw {} B",
            delta.len(),
            raw.len()
        );
        let choice = best_codec(&space, &states, &ctx);
        assert_eq!(choice.codec, StateCodec::Delta);
        assert_eq!(choice.bytes, delta);
    }

    #[test]
    fn best_codec_never_returns_a_stream_larger_than_raw() {
        let spaces = [
            StateSpace::from_radices(&[]).unwrap(),
            StateSpace::from_radices(&[4, 4, 4]).unwrap(),
            StateSpace::from_radices(&[1 << 20, 7]).unwrap(),
            StateSpace::permutation(6).unwrap(),
            StateSpace::from_radices(&[1 << 31, 1 << 31, 1 << 31]).unwrap(),
        ];
        let ctx = Conditioning::default();
        for space in &spaces {
            let states: Vec<State> = {
                let mut v = vec![space.zeros(), space.max()];
                let mut seed = 0x0f0f_0f0f_1234_5678u64;
                for _ in 0..8 {
                    v.push(random_state(space, &mut seed));
                }
                v
            };
            let choice = best_codec(space, &states, &ctx);
            assert!(
                choice.size() <= choice.cost.size(StateCodec::Raw),
                "{} exceeds raw in {space:?}: {}",
                choice.explain(),
                choice.size()
            );
            // The reported winner's size is the winner's real size.
            assert_eq!(choice.size(), choice.cost.size(choice.codec));
        }
    }

    #[test]
    fn out_of_range_states_and_bytes_are_rejected_without_panicking() {
        let space = StateSpace::from_radices(&[3]).unwrap();
        let ctx = Conditioning::default();

        // An invalid state is refused by encode, not turned into a sentinel.
        assert_eq!(space.state(&[3]), None);
        assert_eq!(space.state(&[0, 0]), None);
        let bad = State::from_digits(vec![3]);
        assert_eq!(StateCodec::Raw.encode_state(&space, &bad, &ctx), None);
        assert_eq!(StateCodec::Varint.encode_state(&space, &bad, &ctx), None);
        assert_eq!(StateCodec::Rank.encode_state(&space, &bad, &ctx), None);
        assert_eq!(StateCodec::BlockRank.encode_state(&space, &bad, &ctx), None);

        // Forged bytes are rejected, never panicked on.
        assert_eq!(StateCodec::Raw.decode_state(&space, &[3], &ctx), None);
        assert_eq!(StateCodec::Raw.decode_state(&space, &[0, 0], &ctx), None); // trailing
        assert_eq!(StateCodec::Raw.decode_state(&space, &[], &ctx), None); // truncated
        assert_eq!(StateCodec::Varint.decode_state(&space, &[5], &ctx), None);
        assert_eq!(StateCodec::Varint.decode_state(&space, &[0x80], &ctx), None); // truncated
        assert_eq!(StateCodec::Rank.decode_state(&space, &[5], &ctx), None); // rank >= 3
        assert_eq!(StateCodec::BlockRank.decode_state(&space, &[5], &ctx), None);
        assert_eq!(StateCodec::Rans.decode_state(&space, &[0, 0], &ctx), None); // short model
        assert_eq!(StateCodec::Rans.decode_state(&space, &[], &ctx), None);

        // A predecessor of the wrong shape is refused by Delta, not zero-filled.
        let bad_prior = Conditioning {
            previous: Some(&State::from_digits(vec![0, 0])),
            ..Default::default()
        };
        assert_eq!(
            StateCodec::Delta.encode_state(&space, &space.zeros(), &bad_prior),
            None
        );

        // A space too large for rANS is not a candidate at all, rather than a
        // panic from the table constructor.
        let wide = StateSpace::from_radices(&[5000]).unwrap();
        assert!(wide.rans_alphabet().is_none());
        assert_eq!(
            StateCodec::Rans.encode_state(&wide, &wide.zeros(), &ctx),
            None
        );
        let wide_choice = best_codec(&wide, &[wide.zeros()], &ctx);
        assert_ne!(wide_choice.codec, StateCodec::Rans);
        assert!(wide_choice.size() <= wide_choice.cost.size(StateCodec::Raw));
    }

    #[test]
    fn the_documented_shapes_have_the_documented_spaces() {
        // Template slots: one dimension per slot, the slot's variant count.
        let slots = StateSpace::template_slots(&[4, 4, 4]).unwrap();
        assert_eq!(slots.radices(), &[4, 4, 4]);
        assert_eq!(slots.size(), Some(64));

        // Optional fields: one radix-2 dimension each, the subset as a bitmask.
        let fields = StateSpace::optional_fields(3);
        assert_eq!(fields.radices(), &[2, 2, 2]);
        let set = fields.flags_state(0b101).unwrap();
        assert_eq!(set.digits(), &[1, 0, 1]);
        assert_eq!(fields.flags(&set), Some(0b101));
        assert_eq!(fields.flags_state(0b1000), None);

        // A bounded count.
        assert_eq!(StateSpace::bounded_count(7).unwrap().radices(), &[7]);

        // A permutation, through its factoradic digits, as a real permutation.
        let perms = StateSpace::permutation(5).unwrap();
        assert_eq!(perms.size(), Some(120));
        let perm = [3u32, 0, 4, 1, 2];
        let pst = perms.permutation_state(&perm).unwrap();
        assert_eq!(perms.as_permutation(&pst), Some(perm.to_vec()));
        // The maximal state is a genuine permutation too.
        assert!(perms.as_permutation(&perms.max()).is_some());

        // A subset, through its colex rank.
        let subs = StateSpace::subset(6, 3).unwrap();
        assert_eq!(subs.size(), Some(20));
        let set = [1u32, 3, 5];
        let sst = subs.subset_state(&set).unwrap();
        assert_eq!(subs.as_subset(&sst), Some(set.to_vec()));

        // What is *not* modelled is refused rather than approximated.
        assert!(StateSpace::permutation(21).is_none());
        assert!(StateSpace::subset(64, 32).is_none()); // C(64,32) way past u32
    }
}
