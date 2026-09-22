//! Phase 14.5-14.6: target-directed search, bounded, with the real cost as judge.
//!
//! The search runs *backward from the exact target*, never forward from a random
//! program, because a forward search spends almost all of its budget on programs
//! that could not reconstruct the target at all. Each operator supplies an exact
//! decomposer:
//!
//! ```text
//! Repeat    prove target = repeated(child)          -> real, here
//! Slice     prove target = an exact source slice    -> real, here
//! Ref       prove target = existing material       -> real, here
//! Concat    propose meaningful target partitions    -> real, here (bounded)
//! Patch     derive the exact mismatch residual      -> real, here
//! Template  derive the exact slot values            -> STUB (returns None)
//! ```
//!
//! `Template` is an honest stub: deriving slot values needs a corpus of shared
//! skeletons and slot-inference machinery this module does not have. It returns
//! `None` (no candidate) rather than fabricating a slot assignment, because a
//! fabricated assignment that failed the independent materialization test would
//! teach the search nothing and a fabricated one that passed would be a
//! coincidence, not a proof.
//!
//! **Every completed candidate is verified by independent materialization.** A
//! decomposer's proof is an optimisation, not the authority: the candidate is
//! executed and compared to the target byte for byte before it may become the
//! incumbent. A decomposer built on a wrong assumption is therefore a wasted
//! candidate, never a wrong answer.
//!
//! **Cost authority.** Selection is on `serialize(program).len()` — the actual
//! complete serialized program bytes — never AST node count, coverage or a
//! heuristic score. A `Patch` additionally charges the bytes of its residual,
//! which the plan charges in its own stream (§4.4); both halves are counted so a
//! patch cannot look free by hiding its innovation.
//!
//! **The driver.** `f = g + h` with `Literal(target)` as the incumbent from the
//! first step, pruning when `g + h >= incumbent`. `g` is the serialized cost
//! already fixed; `h` is an admissible lower bound for the unresolved holes.
//!
//! *Admissibility argument for `h`.* The default `h` is **0**. Costs here are
//! byte counts and therefore non-negative, and the true remaining cost of any
//! unresolved region is a sum of non-negative node encodings, so `0` is never an
//! overestimate: `h <= h*` for every completion `h*`. This is the strongest bound
//! we can state without assuming a specific completion shape. A positive
//! per-hole bound would have to prove a *minimum* node encoding, but a hole can be
//! covered by a single cheapest node (`Ref{Material}`, or a shared node already
//! paid for) whose size we cannot bound below without excluding legal programs, so
//! any positive constant would risk inadmissibility. With `h = 0` the driver is a
//! **best-first branch-and-bound** rather than strict A*: `f = g`, ordered by
//! fixed cost, and the incumbent still prunes every partial whose fixed cost has
//! already reached it. We state this rather than inventing a bound we cannot
//! justify.
//!
//! **Determinism.** Identical inputs give identical output. The frontier is
//! ordered by declared operator order (or by fixed cost under [`Strategy::AStar`],
//! with declaration order as the tie-break), the memo is a `BTreeMap`, and no
//! wall-clock reading influences control flow. Elapsed time is *measured* for the
//! economics report but never consulted to decide which program is returned.
//!
//! **Search economics (§14.39).** A search that saves 200 bytes but costs hours of
//! judged compression work is not automatically valuable, so every run reports its
//! candidate count, search time, best gain, gain per search-second and gain per
//! byte of program code the search had to emit. The search takes an explicit work
//! budget and returns the best program found when the budget is exhausted, setting
//! [`SearchReport::exhausted_budget`].

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use super::execute::{execute, ExecContext};
use super::program::{Node, Op, Program};
use super::residual::{derive, EditOp, Residual, ResidualKind};
use super::serialize::serialize;
use super::types::{LiteralId, NodeId, RefTarget, ResidualId, Span};
use super::Bounds;

/// Most `Concat` partitions proposed for one region. Bounded, deliberately not
/// all `n` splits: the plan's `Concat` decomposer must "propose meaningful
/// partitions", and an unbounded split set turns one target into an `O(n^2)`
/// frontier for no demonstrated codelength gain.
const MAX_SPLITS: usize = 12;

/// Regions with at most this many internal positions are split exhaustively,
/// because for a short region the exhaustive set is already within [`MAX_SPLITS`]
/// and being exhaustive removes a way for pruning to lose the optimum. Longer
/// regions fall back to the meaningful proposals below.
const EXHAUSTIVE_SPLIT_LIMIT: usize = 12;

/// Which frontier discipline the driver uses. Both apply the same incumbent and
/// the same `g + h >= incumbent` prune; they differ only in expansion order, and
/// with `h = 0` that difference cannot change the returned optimum once the
/// budget permits full exploration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Expand the cheapest candidate first (`f = g + h`, `h = 0`). Best-first.
    AStar,
    /// Expand in declared operator order; prune with the same bound.
    BranchAndBound,
}

/// A hard, deterministic work budget.
///
/// Deliberately a *work* budget and not a wall-clock one: a wall-clock cutoff
/// would make the returned program depend on machine load, breaking the
/// determinism the plan requires. `max_steps` bounds recursive region solves and
/// `max_candidates` bounds verified candidate programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Maximum region-solve invocations before the search stops.
    pub max_steps: u64,
    /// Maximum candidates generated (and possibly verified) before it stops.
    pub max_candidates: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            max_steps: 2_000,
            max_candidates: 20_000,
        }
    }
}

/// Strategy plus budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchConfig {
    pub strategy: Strategy,
    pub budget: Budget,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig {
            strategy: Strategy::BranchAndBound,
            budget: Budget::default(),
        }
    }
}

/// A complete program together with the residual pool its `Patch` nodes name, and
/// the two cost halves that make up its description.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// The program. Its `Patch` nodes index [`Candidate::residuals`].
    pub program: Program,
    /// Residual pool, in `ResidualId` order.
    pub residuals: Vec<Residual>,
    /// `serialize(program).len()` — the plan's cost authority, in bytes.
    pub program_bytes: u64,
    /// Conservative wire length of the residual stream (§4.4), in bytes.
    pub residual_bytes: u64,
    /// Output length this candidate is intended to cover, kept only so internal
    /// `Concat` spans are honest. Never used for selection.
    pub region_len: u64,
}

impl Candidate {
    /// Build a candidate and compute its two cost halves.
    pub fn new(program: Program, residuals: Vec<Residual>, region_len: u64) -> Self {
        let program_bytes = serialize(&program).len() as u64;
        let residual_bytes = residuals.iter().map(residual_wire_len).sum();
        Candidate {
            program,
            residuals,
            program_bytes,
            residual_bytes,
            region_len,
        }
    }

    /// The complete charged description: program bytes plus residual bytes.
    ///
    /// Selection compares this, and its program half is exactly
    /// `serialize(program).len()`.
    pub fn total_bytes(&self) -> u64 {
        self.program_bytes + self.residual_bytes
    }

    /// The universal fallback: the raw target in one literal node.
    fn literal(bytes: &[u8]) -> Self {
        Candidate::new(literal_program(bytes), Vec::new(), bytes.len() as u64)
    }
}

/// A literal-only program for `bytes`.
fn literal_program(bytes: &[u8]) -> Program {
    Program {
        nodes: vec![Node {
            op: Op::Literal(LiteralId(0)),
            span: Span {
                from: 0,
                len: bytes.len() as u64,
            },
        }],
        literals: vec![bytes.to_vec()],
        root: NodeId(0),
    }
}

/// Length of the unsigned LEB128 encoding of `v`, matching the serializer's
/// variable-length integers.
fn varint_len(v: u64) -> u64 {
    let mut n = 1u64;
    let mut v = v;
    while v >= 0x80 {
        v >>= 7;
        n += 1;
    }
    n
}

/// Conservative wire length of a residual. There is no residual serializer in the
/// VM yet, so this is our honest accounting of the innovation stream: every field
/// is charged, and positions are charged as ascending deltas, the way a real coder
/// would. It is deliberately an over-estimate rather than an under-estimate,
/// because under-charging a `Patch` is how a patch would look free.
fn residual_wire_len(r: &Residual) -> u64 {
    match r {
        Residual::None => 1,
        Residual::SparseSubstitute { positions, values } => {
            let mut n = 1 + varint_len(values.len() as u64) + values.len() as u64;
            let mut prev = 0u64;
            for &p in positions {
                let p = p as u64;
                n += varint_len(p.saturating_sub(prev));
                prev = p;
            }
            n
        }
        Residual::RangeReplace { ranges, data } => {
            let mut n = 1 + varint_len(ranges.len() as u64) + data.len() as u64;
            for &(from, old_len, new_len) in ranges {
                n += varint_len(from as u64)
                    + varint_len(old_len as u64)
                    + varint_len(new_len as u64);
            }
            n
        }
        Residual::RunPatch { runs } => {
            let mut n = 1 + varint_len(runs.len() as u64);
            for &(start, len, _) in runs {
                n += varint_len(start as u64) + varint_len(len as u64) + 1;
            }
            n
        }
        Residual::RankedMismatch { mask_rank, values } => {
            1 + varint_len(*mask_rank) + values.len() as u64
        }
        Residual::EditScript { ops } => {
            let mut n = 1 + varint_len(ops.len() as u64);
            for op in ops {
                n += match op {
                    EditOp::Copy { from, len } => {
                        1 + varint_len(*from as u64) + varint_len(*len as u64)
                    }
                    EditOp::Insert(data) => 1 + varint_len(data.len() as u64) + data.len() as u64,
                };
            }
            n
        }
    }
}

// ---------------------------------------------------------------------------
// Exact backward decomposers
// ---------------------------------------------------------------------------

/// First index at which `needle` occurs in `hay`, or `None`.
fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Longest `l` such that the last `l` bytes of `a` equal the last `l` bytes of `b`.
fn longest_common_suffix(a: &[u8], b: &[u8]) -> usize {
    let max = a.len().min(b.len());
    let mut l = 0;
    while l < max && a[a.len() - 1 - l] == b[b.len() - 1 - l] {
        l += 1;
    }
    l
}

fn is_periodic(target: &[u8], period: usize) -> bool {
    if period == 0 {
        return false;
    }
    (period..target.len()).all(|i| target[i] == target[i - period])
}

/// Every `Ref` candidate: material entries that are exactly `target`.
fn ref_candidates(target: &[u8], material: &[&[u8]]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (i, m) in material.iter().enumerate() {
        if *m == target {
            out.push(Candidate::new(
                Program {
                    nodes: vec![Node {
                        op: Op::Ref {
                            target: RefTarget::Material(i as u32),
                        },
                        span: Span {
                            from: 0,
                            len: target.len() as u64,
                        },
                    }],
                    literals: vec![],
                    root: NodeId(0),
                },
                Vec::new(),
                target.len() as u64,
            ));
        }
    }
    out
}

/// Every `Slice` candidate: `target` is a contiguous range of some material entry.
/// The `Slice` reads a `Ref` node, so an external material slice costs two nodes.
fn slice_candidates(target: &[u8], material: &[&[u8]]) -> Vec<Candidate> {
    let mut out = Vec::new();
    if target.len() > u32::MAX as usize {
        return out;
    }
    for (i, m) in material.iter().enumerate() {
        if m.len() > u32::MAX as usize {
            continue;
        }
        if let Some(from) = find_subslice(m, target) {
            out.push(Candidate::new(
                Program {
                    nodes: vec![
                        Node {
                            op: Op::Ref {
                                target: RefTarget::Material(i as u32),
                            },
                            span: Span {
                                from: 0,
                                len: m.len() as u64,
                            },
                        },
                        Node {
                            op: Op::Slice {
                                src: NodeId(0),
                                from: from as u32,
                                len: target.len() as u32,
                            },
                            span: Span {
                                from: 0,
                                len: target.len() as u64,
                            },
                        },
                    ],
                    literals: vec![],
                    root: NodeId(1),
                },
                Vec::new(),
                target.len() as u64,
            ));
        }
    }
    out
}

/// The cheapest leaf explanation of `region`: a literal, or a `Ref`/`Slice` of
/// available material when that is smaller. Used for `Repeat` children so a
/// repeated block that is itself available material can be referenced, not spelled.
fn best_leaf(region: &[u8], material: &[&[u8]]) -> Candidate {
    let mut best = Candidate::literal(region);
    for c in ref_candidates(region, material)
        .into_iter()
        .chain(slice_candidates(region, material))
    {
        if c.total_bytes() < best.total_bytes() {
            best = c;
        }
    }
    best
}

/// Every `Repeat` candidate: `target` is `child` repeated `count >= 2` times.
fn repeat_candidates(target: &[u8], material: &[&[u8]]) -> Vec<Candidate> {
    let n = target.len();
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    for period in 1..=n / 2 {
        if n % period != 0 || !is_periodic(target, period) {
            continue;
        }
        let count = (n / period) as u32;
        if count < 2 {
            continue;
        }
        let child = best_leaf(&target[..period], material);
        let mut program = child.program.clone();
        let child_root = program.root;
        let root = program.nodes.len() as u32;
        program.nodes.push(Node {
            op: Op::Repeat {
                child: child_root,
                count,
            },
            span: Span {
                from: 0,
                len: n as u64,
            },
        });
        program.root = NodeId(root);
        out.push(Candidate::new(program, child.residuals.clone(), n as u64));
    }
    out
}

/// Is a residual small enough that its base is plausibly "close to the target"?
/// A patch whose residual is most of the target is not an explanation, so it is
/// filtered out rather than offered as a candidate the driver would reject on cost
/// after doing the work.
fn residual_is_close(r: &Residual, wire: u64, n: usize) -> bool {
    let n = n as u64;
    match r {
        Residual::None => false,
        Residual::SparseSubstitute { values, .. } | Residual::RankedMismatch { values, .. } => {
            (values.len() as u64) <= n / 4 && wire <= n
        }
        Residual::RunPatch { runs } => (runs.len() as u64) <= n / 4 && wire <= n,
        Residual::RangeReplace { data, .. } => (data.len() as u64) <= n / 2,
        Residual::EditScript { ops } => {
            let changed: u64 = ops
                .iter()
                .map(|op| match op {
                    EditOp::Insert(d) => d.len() as u64,
                    EditOp::Copy { .. } => 0,
                })
                .sum();
            changed <= n / 2
        }
    }
}

/// Every `Patch` candidate: for each material base whose materialization is close
/// to `target`, derive the exact residual of each kind and offer the smallest.
fn patch_candidates(target: &[u8], material: &[&[u8]]) -> Vec<Candidate> {
    let n = target.len();
    let kinds = [
        ResidualKind::SparseSubstitute,
        ResidualKind::RunPatch,
        ResidualKind::RankedMismatch,
        ResidualKind::RangeReplace,
        ResidualKind::EditScript,
    ];
    let mut out = Vec::new();
    for (i, m) in material.iter().enumerate() {
        for kind in kinds {
            let Some(r) = derive(m, target, kind) else {
                continue;
            };
            let wire = residual_wire_len(&r);
            if !residual_is_close(&r, wire, n) {
                continue;
            }
            out.push(Candidate::new(
                Program {
                    nodes: vec![
                        Node {
                            op: Op::Ref {
                                target: RefTarget::Material(i as u32),
                            },
                            span: Span {
                                from: 0,
                                len: m.len() as u64,
                            },
                        },
                        Node {
                            op: Op::Patch {
                                base: NodeId(0),
                                residual: ResidualId(0),
                            },
                            span: Span {
                                from: 0,
                                len: n as u64,
                            },
                        },
                    ],
                    literals: vec![],
                    root: NodeId(1),
                },
                vec![r],
                n as u64,
            ));
        }
    }
    out
}

/// The cheapest `Repeat` candidate, or `None` when the target is not a repetition.
///
/// Proof obligation: `execute` of the returned program must equal `target`. The
/// decomposer proves the periodicity, but the driver re-proves it by executing.
pub fn decompose_repeat(target: &[u8], material: &[&[u8]]) -> Option<Candidate> {
    repeat_candidates(target, material)
        .into_iter()
        .min_by_key(|c| c.total_bytes())
}

/// The cheapest `Slice` candidate, or `None` when the target is not a contiguous
/// range of any available material.
pub fn decompose_slice(target: &[u8], material: &[&[u8]]) -> Option<Candidate> {
    slice_candidates(target, material)
        .into_iter()
        .min_by_key(|c| c.total_bytes())
}

/// The cheapest `Ref` candidate, or `None` when no material equals the target.
pub fn decompose_ref(target: &[u8], material: &[&[u8]]) -> Option<Candidate> {
    ref_candidates(target, material)
        .into_iter()
        .min_by_key(|c| c.total_bytes())
}

/// The cheapest `Patch` candidate, or `None` when no base is close enough to the
/// target for a derived residual to be an explanation.
pub fn decompose_patch(target: &[u8], material: &[&[u8]]) -> Option<Candidate> {
    patch_candidates(target, material)
        .into_iter()
        .min_by_key(|c| c.total_bytes())
}

/// The `Template` decomposer.
///
/// **Stub, and honest about it.** Deriving exact slot values requires a corpus of
/// shared skeletons plus slot-inference machinery (align the target against a
/// skeleton, solve for each slot) that this module does not have, and inventing a
/// slot assignment would be a guess the independent materialization test would
/// almost always reject. Returning `None` is the truthful answer: no `Template`
/// candidate is proposed until that machinery lands.
pub fn decompose_template(_target: &[u8], _material: &[&[u8]]) -> Option<Candidate> {
    None
}

/// Byte class, used to prefer `Concat` splits at lexical boundaries (whitespace /
/// letters / digits / punctuation) over arbitrary offsets.
fn byte_class(b: u8) -> u8 {
    match b {
        b' ' | b'\t' | b'\n' | b'\r' => 0,
        b'a'..=b'z' => 1,
        b'A'..=b'Z' => 2,
        b'0'..=b'9' => 3,
        _ => 4,
    }
}

/// Evenly sample `k` entries out of an ascending `v`, keeping endpoints.
fn subsample(v: &[usize], k: usize) -> Vec<usize> {
    if v.len() <= k {
        return v.to_vec();
    }
    if k <= 1 {
        return vec![v[0]];
    }
    let mut out = Vec::with_capacity(k);
    for j in 0..k {
        out.push(v[j * (v.len() - 1) / (k - 1)]);
    }
    out.dedup();
    out
}

/// Meaningful `Concat` partitions of `target`: the split index `k` means
/// `target == target[..k].materialize() ++ target[k..].materialize()`.
///
/// Bounded (never all `n` splits for a long region) and meaningful: a short region
/// is split exhaustively because that is both cheap and removes a way for pruning
/// to lose the optimum; a long region proposes only lexical class boundaries and
/// offsets where the right-hand side is a suffix of available material (so the
/// right side can be answered by `Slice`), topped up with evenly spaced offsets to
/// guarantee some proposals.
pub fn decompose_concat_splits(target: &[u8], material: &[&[u8]]) -> Vec<usize> {
    let n = target.len();
    if n < 2 {
        return Vec::new();
    }
    let mut pts: BTreeSet<usize> = BTreeSet::new();
    if n - 1 <= EXHAUSTIVE_SPLIT_LIMIT {
        pts.extend(1..n);
    } else {
        for k in 1..n {
            if byte_class(target[k - 1]) != byte_class(target[k]) {
                pts.insert(k);
            }
        }
        for m in material {
            let l = longest_common_suffix(target, m);
            if l > 0 && l < n {
                pts.insert(n - l);
            }
        }
        let step = (n / (MAX_SPLITS + 1)).max(1);
        let mut k = step;
        while k < n {
            pts.insert(k);
            k += step;
        }
    }
    let v: Vec<usize> = pts.into_iter().collect();
    subsample(&v, MAX_SPLITS)
}

// ---------------------------------------------------------------------------
// The bounded driver
// ---------------------------------------------------------------------------

/// Merge two candidates with a `Concat` node: append `right`'s nodes after
/// `left`'s (ids offset), remap literal ids by content so sharing is preserved,
/// offset residual ids, and make the new node the root.
fn concat_candidates(left: &Candidate, right: &Candidate) -> Candidate {
    let mut literals = left.program.literals.clone();
    let mut lit_map = Vec::with_capacity(right.program.literals.len());
    for lit in &right.program.literals {
        let idx = match literals.iter().position(|x| x == lit) {
            Some(i) => i as u32,
            None => {
                literals.push(lit.clone());
                (literals.len() - 1) as u32
            }
        };
        lit_map.push(idx);
    }

    let node_off = left.program.nodes.len() as u32;
    let res_off = left.residuals.len() as u32;
    let mut nodes = left.program.nodes.clone();
    for node in &right.program.nodes {
        nodes.push(Node {
            op: remap_op(&node.op, node_off, res_off, &lit_map),
            span: node.span,
        });
    }

    let left_root = left.program.root;
    let right_root = NodeId(right.program.root.0.saturating_add(node_off));
    let region_len = left.region_len.saturating_add(right.region_len);
    let root = nodes.len() as u32;
    nodes.push(Node {
        op: Op::Concat {
            parts: [left_root, right_root],
        },
        span: Span {
            from: 0,
            len: region_len,
        },
    });

    let mut residuals = left.residuals.clone();
    residuals.extend(right.residuals.iter().cloned());
    Candidate::new(
        Program {
            nodes,
            literals,
            root: NodeId(root),
        },
        residuals,
        region_len,
    )
}

/// Rebase one operator from a right-hand program into a merged program.
fn remap_op(op: &Op, node_off: u32, res_off: u32, lit_map: &[u32]) -> Op {
    match op {
        Op::Literal(LiteralId(i)) => Op::Literal(LiteralId(lit_map[*i as usize])),
        Op::Concat { parts } => Op::Concat {
            parts: [
                NodeId(parts[0].0.saturating_add(node_off)),
                NodeId(parts[1].0.saturating_add(node_off)),
            ],
        },
        Op::Repeat { child, count } => Op::Repeat {
            child: NodeId(child.0.saturating_add(node_off)),
            count: *count,
        },
        Op::Ref { target } => Op::Ref {
            target: match target {
                RefTarget::Node(n) => RefTarget::Node(NodeId(n.0.saturating_add(node_off))),
                RefTarget::Material(i) => RefTarget::Material(*i),
                RefTarget::Slot(i) => RefTarget::Slot(*i),
            },
        },
        Op::Slice { src, from, len } => Op::Slice {
            src: NodeId(src.0.saturating_add(node_off)),
            from: *from,
            len: *len,
        },
        Op::Template { program, slots } => Op::Template {
            program: NodeId(program.0.saturating_add(node_off)),
            slots: slots
                .iter()
                .map(|s| NodeId(s.0.saturating_add(node_off)))
                .collect(),
        },
        Op::Patch { base, residual } => Op::Patch {
            base: NodeId(base.0.saturating_add(node_off)),
            residual: ResidualId(residual.0.saturating_add(res_off)),
        },
    }
}

/// What a search run found, and what it cost to find it.
#[derive(Clone, Debug)]
pub struct SearchReport {
    /// The best verified program. Always materializes the target exactly.
    pub best: Candidate,
    pub strategy: Strategy,
    /// Candidate programs generated (including those pruned by cost).
    pub candidates: u64,
    /// Candidates discarded by `g + h >= incumbent` before verification.
    pub pruned: u64,
    /// Region solves attempted.
    pub steps: u64,
    /// Wall-clock time. Measured only; never used to choose `best`.
    pub elapsed: Duration,
    /// The literal it had to beat, in complete bytes.
    pub literal_bytes: u64,
    /// Whether the work budget ran out before the search settled.
    pub exhausted_budget: bool,
}

impl SearchReport {
    /// Bytes saved over the literal, saturating at zero (the incumbent is never
    /// lost, so this is never negative).
    pub fn gain(&self) -> u64 {
        self.literal_bytes.saturating_sub(self.best.total_bytes())
    }

    /// Gain per wall-clock second. Timing is informational; it never selects.
    pub fn gain_per_second(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs <= 0.0 {
            self.gain() as f64
        } else {
            self.gain() as f64 / secs
        }
    }

    /// Gain per byte of program code the search had to emit to obtain the gain.
    pub fn gain_per_code_byte(&self) -> f64 {
        if self.best.program_bytes == 0 {
            self.gain() as f64
        } else {
            self.gain() as f64 / self.best.program_bytes as f64
        }
    }
}

struct Searcher<'a> {
    material: &'a [&'a [u8]],
    strategy: Strategy,
    budget: Budget,
    bounds: Bounds,
    /// Deterministic memo keyed by region content. BTreeMap, never HashMap, so
    /// iteration order cannot leak into the result.
    memo: BTreeMap<Vec<u8>, Candidate>,
    steps: u64,
    candidates: u64,
    pruned: u64,
    exhausted: bool,
}

impl<'a> Searcher<'a> {
    fn new(material: &'a [&'a [u8]], config: &SearchConfig, bounds: Bounds) -> Self {
        Searcher {
            material,
            strategy: config.strategy,
            budget: config.budget,
            bounds,
            memo: BTreeMap::new(),
            steps: 0,
            candidates: 0,
            pruned: 0,
            exhausted: false,
        }
    }

    /// Independent authority: materialize the candidate and compare byte for byte.
    fn verify(&self, c: &Candidate, region: &[u8]) -> bool {
        let cx = ExecContext {
            refs: self.material.to_vec(),
            residuals: c.residuals.clone(),
        };
        matches!(
            execute(&c.program, &cx, &self.bounds),
            Ok(out) if out.as_slice() == region
        )
    }

    /// The best program this searcher can find for `region`.
    ///
    /// The incumbent starts as `Literal(region)`, so the return value is never
    /// worse than the literal, and a non-improving region simply returns it. If the
    /// budget is exhausted the best found so far is returned and the caller is told.
    fn solve(&mut self, region: &[u8]) -> Candidate {
        if let Some(c) = self.memo.get(region) {
            return c.clone();
        }
        self.steps += 1;
        if self.steps > self.budget.max_steps {
            self.exhausted = true;
            return Candidate::literal(region);
        }

        let mut incumbent = Candidate::literal(region);

        // Leaf decomposers. Literal is the incumbent, so it is not re-proposed.
        let mut leaves: Vec<Candidate> = Vec::new();
        leaves.extend(ref_candidates(region, self.material));
        leaves.extend(slice_candidates(region, self.material));
        leaves.extend(repeat_candidates(region, self.material));
        leaves.extend(patch_candidates(region, self.material));
        leaves.extend(decompose_template(region, self.material));
        if self.strategy == Strategy::AStar {
            // Best-first: cheapest fixed cost first. Stable, so ties keep
            // declaration order and the result is deterministic.
            leaves.sort_by_key(|c| c.total_bytes());
        }

        for c in leaves {
            if self.candidates >= self.budget.max_candidates {
                self.exhausted = true;
                break;
            }
            self.candidates += 1;
            let g = c.total_bytes();
            let h = 0u64;
            if g + h >= incumbent.total_bytes() {
                self.pruned += 1;
                continue;
            }
            if self.verify(&c, region) {
                incumbent = c;
            }
        }

        // Concat: bounded, meaningful partitions with branch-and-bound on the
        // incumbent. A split is only worth solving if both halves plus the join
        // could still fit under the current bound, which is what the prune checks.
        if !self.exhausted {
            for k in decompose_concat_splits(region, self.material) {
                if self.exhausted || self.candidates >= self.budget.max_candidates {
                    self.exhausted = self.candidates >= self.budget.max_candidates;
                    break;
                }
                let left = self.solve(&region[..k]);
                if self.exhausted {
                    break;
                }
                let right = self.solve(&region[k..]);
                if self.exhausted {
                    break;
                }
                let c = concat_candidates(&left, &right);
                self.candidates += 1;
                let g = c.total_bytes();
                let h = 0u64;
                if g + h >= incumbent.total_bytes() {
                    self.pruned += 1;
                    continue;
                }
                if self.verify(&c, region) {
                    incumbent = c;
                }
            }
        }

        if !self.exhausted {
            self.memo.insert(region.to_vec(), incumbent.clone());
        }
        incumbent
    }
}

/// Bounds an execution must satisfy to count as a verification of the target.
fn exec_bounds(target_len: usize) -> Bounds {
    let mut b = Bounds::default();
    b.max_output = (target_len as u64).saturating_mul(4).max(1 << 20);
    b
}

/// Search backward from `target` for the cheapest program that materializes it,
/// using `material` as the caller-available reference material.
///
/// The returned program is verified by execution inside the search, but the caller
/// may execute it again from [`SearchReport::best`] with the reported residual pool;
/// doing so is expected to reproduce `target` byte for byte.
pub fn search(target: &[u8], material: &[&[u8]], config: &SearchConfig) -> SearchReport {
    let started = Instant::now();
    let bounds = exec_bounds(target.len());
    let mut searcher = Searcher::new(material, config, bounds);
    let literal_bytes = Candidate::literal(target).total_bytes();

    let best = searcher.solve(target);
    debug_assert!(
        searcher.verify(&best, target),
        "the incumbent must always materialize the target"
    );

    SearchReport {
        best,
        strategy: config.strategy,
        candidates: searcher.candidates,
        pruned: searcher.pruned,
        steps: searcher.steps,
        elapsed: started.elapsed(),
        literal_bytes,
        exhausted_budget: searcher.exhausted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procedural::execute::{execute, ExecContext};
    use crate::procedural::program::{Node, Op, Program};
    use crate::procedural::types::{LiteralId, NodeId, Span};
    use crate::procedural::Bounds;

    /// Execute a candidate the way a decoder would, with no proof authority.
    fn materialize(c: &Candidate, material: &[&[u8]]) -> Vec<u8> {
        let cx = ExecContext {
            refs: material.to_vec(),
            residuals: c.residuals.clone(),
        };
        execute(&c.program, &cx, &Bounds::default()).expect("candidate must materialize")
    }

    /// Deterministic pseudo-random bytes: no `rand` dependency, and identical on
    /// every machine, which the determinism test depends on.
    fn lcg_bytes(n: usize, seed: u64) -> Vec<u8> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (s >> 33) as u8
            })
            .collect()
    }

    #[test]
    fn repeat_is_recovered_for_a_repetition_and_beats_the_literal() {
        let target = b"abcabcabcabcabcabc"; // "abc" x 6, 18 bytes
        let report = search(target, &[], &SearchConfig::default());
        assert!(
            report.gain() > 0,
            "a repetition must beat its own literal, gain was {}",
            report.gain()
        );
        assert!(
            report.best.program.nodes.len() >= 2,
            "a Repeat program needs at least a child and the repeat"
        );
        assert!(
            report
                .best
                .program
                .nodes
                .iter()
                .any(|n| matches!(n.op, Op::Repeat { .. })),
            "the recovered program must contain a Repeat"
        );
        assert_eq!(materialize(&report.best, &[]), target);
    }

    #[test]
    fn slice_is_recovered_for_material_substring() {
        // Long enough that two nodes plus the header still beat a literal, and not
        // the whole material entry, so only a Slice can explain it.
        let material: &[&[u8]] = &[b"the quick brown fox"];
        let target = b"quick brown fox";
        let report = search(target, material, &SearchConfig::default());
        assert!(report.gain() > 0, "a slice of material must beat a literal");
        assert!(
            report
                .best
                .program
                .nodes
                .iter()
                .any(|n| matches!(n.op, Op::Slice { .. })),
            "the recovered program must contain a Slice"
        );
        assert_eq!(materialize(&report.best, material), target);
    }

    #[test]
    fn ref_is_recovered_for_exact_material() {
        let material: &[&[u8]] = &[b"shared block of material", b"other"];
        let target = b"shared block of material";
        let report = search(target, material, &SearchConfig::default());
        assert!(
            report.gain() > 0,
            "an exact material reference must beat a literal"
        );
        assert_eq!(report.best.program.nodes.len(), 1);
        assert!(matches!(
            report.best.program.nodes[0].op,
            Op::Ref {
                target: RefTarget::Material(0)
            }
        ));
        assert_eq!(materialize(&report.best, material), target);
    }

    #[test]
    fn patch_derives_the_exact_residual_for_a_close_base() {
        let base = b"the quick brown fox jumps".to_vec();
        let mut target = base.clone();
        target[4] = b'Q';
        target[10] = b'Z';
        target[20] = b'!';
        let material: &[&[u8]] = &[&base];
        let report = search(&target, material, &SearchConfig::default());
        assert!(
            report.gain() > 0,
            "a close base plus a small residual must beat a literal"
        );
        assert!(
            report
                .best
                .program
                .nodes
                .iter()
                .any(|n| matches!(n.op, Op::Patch { .. })),
            "the recovered program must contain a Patch"
        );
        assert_eq!(materialize(&report.best, material), target);
    }

    #[test]
    fn nothing_beats_the_literal_and_the_incumbent_survives() {
        // Distinct bytes: no periodicity, no material, no close base. Nothing can
        // explain this but a literal, so the literal must be returned intact.
        let target: Vec<u8> = (0u8..=15).map(|b| b.wrapping_mul(17)).collect();
        let report = search(&target, &[], &SearchConfig::default());
        assert_eq!(report.gain(), 0);
        assert_eq!(
            serialize(&report.best.program),
            serialize(&literal_program(&target)),
            "the incumbent literal must not be lost when nothing improves on it"
        );
        assert_eq!(materialize(&report.best, &[]), target);
    }

    #[test]
    fn every_returned_program_materializes_the_target_exactly() {
        let material: &[&[u8]] = &[b"aaaa", b"wxyz", b"hello hello hello"];
        let cases: Vec<Vec<u8>> = vec![
            b"x".to_vec(),
            b"aaaaaaaa".to_vec(),
            b"hello hello hello".to_vec(),
            b"yz".to_vec(),
            lcg_bytes(64, 7),
            Vec::new(),
        ];
        for target in &cases {
            let report = search(target, material, &SearchConfig::default());
            assert_eq!(
                materialize(&report.best, material),
                *target,
                "search returned a program that does not reconstruct the target"
            );
            assert!(
                report.best.total_bytes() <= report.literal_bytes,
                "the incumbent was lost: {} > {}",
                report.best.total_bytes(),
                report.literal_bytes
            );
        }
    }

    #[test]
    fn selection_uses_serialized_bytes_not_node_count() {
        // "xy" x 50: the Repeat program has two nodes, the literal has one. A
        // selection that used node count would keep the one-node literal; the bytes
        // authority must keep the two-node Repeat because it is far shorter.
        let target: Vec<u8> = b"xy".iter().copied().cycle().take(100).collect();
        let report = search(&target, &[], &SearchConfig::default());
        assert!(
            report.best.program.nodes.len() > 1,
            "a node-count selection would have stopped at the one-node literal"
        );
        assert!(
            report.best.total_bytes() < report.literal_bytes,
            "the Repeat must be cheaper in serialized bytes than the literal"
        );
        assert_eq!(materialize(&report.best, &[]), target);
    }

    #[test]
    fn bounded_search_matches_brute_force_on_a_small_case() {
        let target = b"abcabcabcabc";
        let material: &[&[u8]] = &[];
        let report = search(target, material, &SearchConfig::default());

        let brute = brute_force_min_bytes(target, material);
        assert!(
            report.best.total_bytes() <= brute,
            "bounded search returned {} bytes but brute force achieved {}",
            report.best.total_bytes(),
            brute
        );
        assert!(
            brute < report.literal_bytes,
            "brute force should beat the literal here"
        );
        assert_eq!(materialize(&report.best, material), target);
    }

    /// Independent exhaustive enumeration over a well-defined universe of small
    /// programs: literals, references, slices, repeats and two-literal concats. It
    /// shares no code with the decomposers, so agreement is evidence that pruning
    /// did not discard the optimum. Returns the cheapest complete serialized bytes.
    fn brute_force_min_bytes(target: &[u8], material: &[&[u8]]) -> u64 {
        let mut best = Candidate::literal(target).total_bytes();
        let mut pool: Vec<Vec<u8>> = vec![target.to_vec()];
        for i in 0..target.len() {
            for j in i + 1..=target.len() {
                pool.push(target[i..j].to_vec());
            }
        }
        for m in material {
            pool.push(m.to_vec());
        }
        pool.sort();
        pool.dedup();

        let consider = |program: Program, best: &mut u64| {
            let c = Candidate::new(program, Vec::new(), target.len() as u64);
            let cx = ExecContext {
                refs: material.to_vec(),
                residuals: vec![],
            };
            if let Ok(out) = execute(&c.program, &cx, &Bounds::default()) {
                if out.as_slice() == target && c.total_bytes() < *best {
                    *best = c.total_bytes();
                }
            }
        };

        let node = |op: Op| Node {
            op,
            span: Span::EMPTY,
        };

        // Literals, references.
        for chunk in &pool {
            if chunk.as_slice() == target {
                consider(
                    Program {
                        nodes: vec![node(Op::Literal(LiteralId(0)))],
                        literals: vec![chunk.clone()],
                        root: NodeId(0),
                    },
                    &mut best,
                );
            }
        }
        for (i, m) in material.iter().enumerate() {
            if *m == target {
                consider(
                    Program {
                        nodes: vec![node(Op::Ref {
                            target: RefTarget::Material(i as u32),
                        })],
                        literals: vec![],
                        root: NodeId(0),
                    },
                    &mut best,
                );
            }
            if let Some(from) = find_subslice(m, target) {
                consider(
                    Program {
                        nodes: vec![
                            node(Op::Ref {
                                target: RefTarget::Material(i as u32),
                            }),
                            node(Op::Slice {
                                src: NodeId(0),
                                from: from as u32,
                                len: target.len() as u32,
                            }),
                        ],
                        literals: vec![],
                        root: NodeId(1),
                    },
                    &mut best,
                );
            }
        }
        // Two-literal concats: every literal a that prefixes the target whose
        // remainder is also in the pool.
        let has = |needle: &[u8]| pool.iter().any(|p| p.as_slice() == needle);
        for a in &pool {
            if a.len() < target.len() && target.starts_with(a) {
                let b = &target[a.len()..];
                if has(b) {
                    consider(
                        Program {
                            nodes: vec![
                                node(Op::Literal(LiteralId(0))),
                                node(Op::Literal(LiteralId(1))),
                                node(Op::Concat {
                                    parts: [NodeId(0), NodeId(1)],
                                }),
                            ],
                            literals: vec![a.clone(), b.to_vec()],
                            root: NodeId(2),
                        },
                        &mut best,
                    );
                }
            }
        }
        // Repeats of a literal child.
        for child in &pool {
            if child.is_empty() || target.len() % child.len() != 0 {
                continue;
            }
            if target.chunks(child.len()).all(|c| c == child.as_slice()) {
                let count = (target.len() / child.len()) as u32;
                consider(
                    Program {
                        nodes: vec![
                            node(Op::Literal(LiteralId(0))),
                            node(Op::Repeat {
                                child: NodeId(0),
                                count,
                            }),
                        ],
                        literals: vec![child.clone()],
                        root: NodeId(1),
                    },
                    &mut best,
                );
            }
        }
        best
    }

    #[test]
    fn the_work_budget_is_respected_and_search_reports_rather_than_hangs() {
        // A large, incompressible target with a deliberately tiny budget: the
        // search must stop and say so, not run to exhaustion.
        let target = lcg_bytes(400, 12345);
        let material_owned = lcg_bytes(200, 999);
        let material: &[&[u8]] = &[&material_owned];
        let config = SearchConfig {
            strategy: Strategy::BranchAndBound,
            budget: Budget {
                max_steps: 2,
                max_candidates: 3,
            },
        };
        let report = search(&target, material, &config);
        assert!(
            report.exhausted_budget,
            "a tiny budget over an expensive target must report exhaustion"
        );
        // The budget must actually bind, not merely be reported. `steps` may reach
        // `max_steps + 1` because the solve that notices the overrun is counted
        // before it returns the literal; `candidates` is checked before each
        // increment, so it never passes the ceiling.
        assert!(
            report.steps <= config.budget.max_steps + 1,
            "the search ran {} region solves for a budget of {}",
            report.steps,
            config.budget.max_steps
        );
        assert!(
            report.candidates <= config.budget.max_candidates,
            "the search generated {} candidates for a budget of {}",
            report.candidates,
            config.budget.max_candidates
        );
        assert_eq!(materialize(&report.best, material), target);
    }

    #[test]
    fn identical_searches_return_identical_programs() {
        let target = b"ababababababababababab"; // periodic, so search does real work
        let material: &[&[u8]] = &[b"abab", b"zzz"];
        let config = SearchConfig::default();
        let a = search(target, material, &config);
        let b = search(target, material, &config);
        assert_eq!(serialize(&a.best.program), serialize(&b.best.program));
        assert_eq!(a.best.residuals, b.best.residuals);
        assert_eq!(a.candidates, b.candidates);
        assert_eq!(a.steps, b.steps);
    }

    #[test]
    fn strategy_does_not_change_the_optimum() {
        let target = b"abcabcabcabcabcabc";
        let material: &[&[u8]] = &[b"abc", b"abcabc"];
        let star = search(
            target,
            material,
            &SearchConfig {
                strategy: Strategy::AStar,
                ..SearchConfig::default()
            },
        );
        let bnb = search(
            target,
            material,
            &SearchConfig {
                strategy: Strategy::BranchAndBound,
                ..SearchConfig::default()
            },
        );
        assert_eq!(star.best.total_bytes(), bnb.best.total_bytes());
        assert_eq!(serialize(&star.best.program), serialize(&bnb.best.program));
    }

    #[test]
    fn the_report_exposes_search_economics() {
        let target = b"abcabcabcabcabcabc";
        let report = search(target, &[], &SearchConfig::default());
        assert!(
            report.candidates > 0,
            "a search must report its candidate count"
        );
        assert!(report.steps > 0);
        assert!(report.gain() > 0);
        assert!(report.gain_per_second() >= 0.0);
        assert!(report.gain_per_code_byte() > 0.0);
        assert_eq!(report.strategy, Strategy::BranchAndBound);
    }

    #[test]
    fn the_template_decomposer_is_an_honest_stub() {
        // It must return no candidate rather than fabricating a slot assignment; a
        // fabricated candidate would be a guess the materialization test would
        // usually reject. This test pins the honesty of the stub.
        assert!(decompose_template(b"anything at all", &[b"anything"]).is_none());
    }

    #[test]
    fn concat_splits_are_bounded_and_meaningful() {
        // A long, lexically mixed target: the proposal must be bounded by MAX_SPLITS
        // and must include at least one whitespace/letter boundary.
        let target = b"alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu";
        let splits = decompose_concat_splits(target, &[]);
        assert!(!splits.is_empty());
        assert!(splits.len() <= MAX_SPLITS, "splits must stay bounded");
        assert!(
            splits
                .iter()
                .any(|&k| byte_class(target[k - 1]) != byte_class(target[k])),
            "at least one proposal must sit on a lexical boundary"
        );
    }
}
