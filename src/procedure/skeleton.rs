//! Skeleton synthesis / anti-unification (§14.14, §14.9).
//!
//! Two representations live here, and the difference between them is the whole
//! point of this revision:
//!
//! * [`anti_unify`] — the byte-level **single-slot** skeleton: a shared common
//!   prefix and common suffix with one differing interior region. This is the
//!   representation the boundary pass tested, kept because it is what the
//!   existing cost plumbing knows how to price.
//! * [`synthesise_dag`] — a **shared DAG** anti-unifier: the cohort is split into
//!   shared segments and holes, every shared segment is stored **once**, and a
//!   repeated sub-skeleton is referenced rather than re-emitted (common
//!   subexpression elimination over the program). §14.9 requires each profitable
//!   node to be stored once; the DAG is where that is enforced and its
//!   [`node_count`](Dag::nodes) is exposed so the saving is visible.
//!
//! ## The minimum useful cohort size and the unshared fallback
//!
//! A singleton (or any cohort below [`DEFAULT_MIN_COHORT`]) cannot share
//! anything. Charging it a program is the worst of both worlds: it pays for a
//! "shared" skeleton and amortises it over one member. [`synthesise_cohort`]
//! therefore returns [`Plan::Unshared`] in that case, and [`synthesise`] maps it
//! to a skeleton whose [`program_bytes`](Skeleton::program_bytes) is **0**: the
//! members are priced directly (through the accepted coder, in the residual
//! stream) and no program is charged at all.
//!
//! ## Base quality
//!
//! A `Patch` decomposition only pays when the base is genuinely close to the
//! member, because the residual is a *patch stream* the accepted coder does not
//! model as well as it models structured text. [`patch_profitable`] is the cheap,
//! honest closeness test: it measures the total canonical residual wire length
//! against the total member bytes and refuses the shared base when the patch
//! stream is not smaller. When it refuses, [`synthesise_cohort`] returns the
//! fallback instead of emitting a Patch whose residual costs more than coding the
//! member directly.
//!
//! ## Why the anti-unifier is byte-level
//!
//! Every target this experiment extracts is a *single* IR token, so its
//! [`crate::ir::Kind`] sequence has length one and there is no finer kind
//! structure inside a target to align to; the only structure cohort members share
//! is their bytes. The `Kind` sequence still does its job where it is
//! informative — as the cohort signature for classes without a name signature.

use std::collections::BTreeMap;

use crate::procedural::execute::{execute, ExecContext};
use crate::procedural::program::{Node, Op, Program};
use crate::procedural::serialize::serialize;
use crate::procedural::types::{LiteralId, NodeId, RefTarget, Span as ProgSpan};
use crate::procedural::Bounds;

use super::cohort::DEFAULT_MIN_COHORT;
use super::member;

/// A synthesised single-slot skeleton, plus the byte strings needed to derive
/// member residuals against its empty-hole base.
#[derive(Clone, Debug, PartialEq)]
pub struct Skeleton {
    pub program: Program,
    /// **Charged** program bytes — normally `serialize(program).len()`. For an
    /// unshared cohort it is deliberately `0`: no program is emitted, and the
    /// members are priced directly. The `program` is then a constant plumbing
    /// shim (not cohort information), so charging it would be dishonest in the
    /// other direction.
    pub program_bytes: usize,
    /// Bytes shared by every member at the front.
    pub prefix: Vec<u8>,
    /// Bytes shared by every member at the back.
    pub suffix: Vec<u8>,
    /// The emitted program's node count (after any DAG sharing).
    pub nodes: usize,
}

impl Skeleton {
    /// The base this skeleton materialises with an empty hole: `prefix ++ suffix`.
    pub fn base(&self) -> Vec<u8> {
        let mut v = self.prefix.clone();
        v.extend_from_slice(&self.suffix);
        v
    }
}

/// Which shared-base strategy a cohort gets.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Large enough, and close enough, to share a program profitably.
    Shared(Skeleton),
    /// No program: the members are priced directly.
    Unshared(UnsharedReason),
}

/// Why a cohort was charged directly rather than shared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsharedReason {
    /// Fewer than the minimum useful cohort size.
    TooSmall,
    /// The chosen base is not close enough for a `Patch` residual to pay.
    BaseNotClose,
}

/// Materialise the empty-hole base through the real VM. This is the *authority*
/// for what the shared program produces; `Skeleton::base` is only a convenience.
pub fn materialise_base(sk: &Skeleton) -> Result<Vec<u8>, String> {
    let empty: &[u8] = b"";
    let cx = ExecContext {
        refs: vec![empty],
        residuals: Vec::new(),
    };
    execute(&sk.program, &cx, &Bounds::default())
        .map_err(|e| format!("procedure: base materialisation failed: {}", e.name()))
}

/// Materialise a skeleton with an explicit slot value (the member's hole). Used
/// by the round-trip verification.
pub fn materialise_with_hole(sk: &Skeleton, hole: &[u8]) -> Result<Vec<u8>, String> {
    let cx = ExecContext {
        refs: vec![hole],
        residuals: Vec::new(),
    };
    execute(&sk.program, &cx, &Bounds::default())
        .map_err(|e| format!("procedure: slot materialisation failed: {}", e.name()))
}

fn common_prefix(members: &[Vec<u8>]) -> Vec<u8> {
    let mut p = members[0].clone();
    for m in &members[1..] {
        let mut k = 0usize;
        while k < p.len() && k < m.len() && p[k] == m[k] {
            k += 1;
        }
        p.truncate(k);
        if p.is_empty() {
            break;
        }
    }
    p
}

fn common_suffix(members: &[Vec<u8>], prefix_len: usize) -> Vec<u8> {
    let min_len = members.iter().map(|m| m.len()).min().unwrap_or(0);
    let cap = min_len.saturating_sub(prefix_len);
    let last = members[0].len();
    let mut s = 0usize;
    while s < cap {
        let b = members[0][last - 1 - s];
        if members.iter().all(|m| m[m.len() - 1 - s] == b) {
            s += 1;
        } else {
            break;
        }
    }
    members[0][last - s..last].to_vec()
}

/// The one-hole `Template` program for a `(prefix, suffix)` anti-unifier.
///
/// Node layout (ids are positional and stable):
///
/// ```text
/// 0: Literal(prefix)
/// 1: Ref{Slot(0)}          <- the differing region
/// 2: Literal(suffix)
/// 3: Concat(0, 1)
/// 4: Concat(3, 2)
/// 5: Ref{Material(0)}      <- the slot's value for this execution
/// 6: Template{ program: 4, slots: [5] }   (root)
/// ```
fn hole_template(prefix: &[u8], suffix: &[u8]) -> Program {
    let literals = vec![prefix.to_vec(), suffix.to_vec()];
    let e = ProgSpan::EMPTY;
    let nodes = vec![
        Node {
            op: Op::Literal(LiteralId(0)),
            span: e,
        },
        Node {
            op: Op::Ref {
                target: RefTarget::Slot(0),
            },
            span: e,
        },
        Node {
            op: Op::Literal(LiteralId(1)),
            span: e,
        },
        Node {
            op: Op::Concat {
                parts: [NodeId(0), NodeId(1)],
            },
            span: e,
        },
        Node {
            op: Op::Concat {
                parts: [NodeId(3), NodeId(2)],
            },
            span: e,
        },
        Node {
            op: Op::Ref {
                target: RefTarget::Material(0),
            },
            span: e,
        },
        Node {
            op: Op::Template {
                program: NodeId(4),
                slots: vec![NodeId(5)],
            },
            span: e,
        },
    ];
    Program {
        nodes,
        literals,
        root: NodeId(6),
    }
}

fn skeleton_from(program: Program, prefix: Vec<u8>, suffix: Vec<u8>, charged: bool) -> Skeleton {
    let nodes = program.nodes.len();
    // Phase 14.29: the *explanation is data*, and §14.29 requires it to be
    // entropy-coded under its own structural model. `best_program_bytes` returns
    // the smaller of the raw canonical serialization and the coded form, so a
    // program that is too small for its own model bytes to pay for themselves is
    // honestly charged the raw size instead of being made to look better.
    let program_bytes = if charged {
        crate::procedural::progcodec::best_program_bytes(&program)
    } else {
        0
    };
    Skeleton {
        program,
        program_bytes,
        prefix,
        suffix,
        nodes,
    }
}

/// The pure single-slot anti-unifier: a common prefix and suffix with one hole.
/// Always shares (never falls back), so it is the right primitive to test, and
/// it is what [`synthesise`] uses for a cohort that *does* share. `members` must
/// be non-empty.
pub fn anti_unify(members: &[Vec<u8>]) -> Skeleton {
    assert!(!members.is_empty(), "anti_unify: empty cohort");
    let prefix = common_prefix(members);
    let suffix = common_suffix(members, prefix.len());
    let program = hole_template(&prefix, &suffix);
    skeleton_from(program, prefix, suffix, true)
}

/// The unshared fallback: a program that carries **no** cohort information and is
/// charged **zero** bytes. The single `Ref{Material(0)}` node is a plumbing shim
/// so the single-hole caller can still materialise a base (the empty string) and
/// a member (its own hole); it is the constant "direct" path, not a skeleton, and
/// it is identical for every unshared cohort. The member bytes are charged in the
/// residual stream through the accepted coder, which is what "priced directly"
/// means.
pub fn unshared() -> Skeleton {
    let program = Program {
        nodes: vec![Node {
            op: Op::Ref {
                target: RefTarget::Material(0),
            },
            span: ProgSpan::EMPTY,
        }],
        literals: Vec::new(),
        root: NodeId(0),
    };
    skeleton_from(program, Vec::new(), Vec::new(), false)
}

/// A one-node program that materialises `bytes` verbatim. Its serialized length
/// is essentially `bytes.len()`, which is the point of the `Literal` control:
/// nothing is shared, so nothing is saved.
pub fn literal(bytes: &[u8]) -> Skeleton {
    let program = Program {
        nodes: vec![Node {
            op: Op::Literal(LiteralId(0)),
            span: ProgSpan::EMPTY,
        }],
        literals: vec![bytes.to_vec()],
        root: NodeId(0),
    };
    skeleton_from(program, bytes.to_vec(), Vec::new(), true)
}

/// The cheap, honest closeness test (§14.14): is a `Patch` against `base`
/// smaller than coding the members directly?
///
/// Both sides are measured in **canonical residual wire bytes** vs **member
/// bytes**. The accepted coder compresses both, so this is a *necessary* test,
/// not the final authority — but it is made on measured sizes, and it refuses the
/// base exactly when the patch stream would be larger than the bytes it replaces.
/// `base` should be non-empty and `members` non-empty.
pub fn patch_profitable(base: &[u8], members: &[Vec<u8>]) -> bool {
    let mut residual = 0u64;
    let mut direct = 0u64;
    for m in members {
        let r = member::best_residual(base, m);
        if !member::verify(base, m, &r) {
            // A base that cannot even round-trip its members is not usable.
            return false;
        }
        residual += member::wire(&r).len() as u64;
        direct += m.len() as u64;
    }
    residual < direct
}

/// Decide a cohort's representation at an explicit minimum size. This is the
/// entry point the cohorting layer should call.
pub fn synthesise_cohort(members: &[Vec<u8>], min_cohort: usize) -> Plan {
    assert!(!members.is_empty(), "synthesise_cohort: empty cohort");
    if members.len() < min_cohort.max(1) {
        return Plan::Unshared(UnsharedReason::TooSmall);
    }
    let sk = anti_unify(members);
    if !patch_profitable(&sk.base(), members) {
        return Plan::Unshared(UnsharedReason::BaseNotClose);
    }
    Plan::Shared(sk)
}

/// Anti-unify a cohort into one shared skeleton at an explicit minimum useful
/// cohort size, falling back to the unshared representation below it (and when
/// the base is not close enough). Always returns a valid [`Skeleton`] so the
/// existing single-hole cost plumbing can price it; an unshared result has
/// `program_bytes == 0`.
pub fn synthesise_with_min(members: &[Vec<u8>], min_cohort: usize) -> Skeleton {
    match synthesise_cohort(members, min_cohort) {
        Plan::Shared(sk) => sk,
        Plan::Unshared(_) => unshared(),
    }
}

/// Anti-unify a cohort into one shared skeleton at [`DEFAULT_MIN_COHORT`],
/// falling back to a directly-charged (no-program) representation when the cohort
/// is too small or its base is not close enough.
pub fn synthesise(members: &[Vec<u8>]) -> Skeleton {
    synthesise_with_min(members, DEFAULT_MIN_COHORT)
}

// ---------------------------------------------------------------------------
// The shared DAG anti-unifier (§14.9, §14.14)
// ---------------------------------------------------------------------------

/// The smallest shared segment worth anchoring a DAG on.
pub const MIN_SHARED: usize = 2;

/// The cap on the lengths considered by the longest-common-substring search, so
/// synthesis stays bounded on a long member. Segments longer than this are found
/// as the concatenation of shorter anchors, and every anchor is otherwise
/// verified against all members, so no correctness is lost — only some
/// optimality.
const LCS_CAP: usize = 256;

/// One element of a cohort's shared structure: a byte segment common to every
/// member, or a hole (whose value differs by member).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Shared(Vec<u8>),
    Hole(usize),
}

/// A shared-DAG skeleton. Every member is `Hole(0) Shared(0) Hole(1) …`, the
/// shared segments are stored **once**, and a repeated sub-skeleton is referenced
/// rather than re-emitted.
///
/// Delivered for the coordinator to wire into the cost path; it is exercised by
/// this module's tests rather than called from the current single-slot caller, so
/// the dead-code lint is silenced deliberately.
#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct Dag {
    pub program: Program,
    /// `serialize(program).len()` — the cost authority for the shared program.
    pub program_bytes: usize,
    pub pieces: Vec<Piece>,
    /// Number of slots (holes).
    pub slots: usize,
    /// Node count of the emitted (DAG-shared) program.
    pub nodes: usize,
    /// Node count the same structure would need with no sharing (a literal node
    /// per occurrence, no common-subexpression elimination). `nodes < naive_nodes`
    /// is exactly the saving sharing bought.
    pub naive_nodes: usize,
    /// Total bytes of shared (non-hole) structure.
    pub shared_bytes: usize,
}

#[allow(dead_code)]
impl Dag {
    /// The per-member hole values, by walking `self.pieces` over `member`.
    /// `None` if `member` does not have the shared structure (which should not
    /// happen for members the DAG was built from).
    pub fn split(&self, member: &[u8]) -> Option<Vec<Vec<u8>>> {
        let mut holes = Vec::new();
        let mut pos = 0usize;
        for (i, p) in self.pieces.iter().enumerate() {
            match p {
                Piece::Shared(seg) => {
                    if !member[pos..].starts_with(seg) {
                        return None;
                    }
                    pos += seg.len();
                }
                Piece::Hole(_) => {
                    let next = self.pieces[i + 1..].iter().find_map(|q| match q {
                        Piece::Shared(s) => Some(s.as_slice()),
                        Piece::Hole(_) => None,
                    });
                    match next {
                        Some(seg) => {
                            let rel = member[pos..].windows(seg.len()).position(|w| w == seg)?;
                            holes.push(member[pos..pos + rel].to_vec());
                            pos += rel;
                        }
                        None => {
                            holes.push(member[pos..].to_vec());
                            pos = member.len();
                        }
                    }
                }
            }
        }
        Some(holes)
    }

    /// Materialise the DAG with explicit hole values (in slot order).
    pub fn materialise(&self, holes: &[Vec<u8>]) -> Result<Vec<u8>, String> {
        if holes.len() != self.slots {
            return Err(format!(
                "procedure: dag needs {} holes, got {}",
                self.slots,
                holes.len()
            ));
        }
        let refs: Vec<&[u8]> = holes.iter().map(|h| h.as_slice()).collect();
        let cx = ExecContext {
            refs,
            residuals: Vec::new(),
        };
        execute(&self.program, &cx, &Bounds::default())
            .map_err(|e| format!("procedure: dag materialisation failed: {}", e.name()))
    }

    /// True iff the DAG reconstructs every member exactly.
    pub fn reconstructs(&self, members: &[Vec<u8>]) -> bool {
        members.iter().all(|m| match self.split(m) {
            Some(h) => self.materialise(&h).as_deref() == Ok(m.as_slice()),
            None => false,
        })
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Longest common substring of `a` and `b`, considering only the first
/// `cap` bytes of each. O(cap²), deterministic (earliest longest match wins).
fn lcs_capped(a: &[u8], b: &[u8], cap: usize) -> Vec<u8> {
    let a = &a[..a.len().min(cap)];
    let b = &b[..b.len().min(cap)];
    let mut prev = vec![0usize; b.len() + 1];
    let mut best_len = 0usize;
    let mut best_end = 0usize;
    for i in 1..=a.len() {
        let mut cur = vec![0usize; b.len() + 1];
        for j in 1..=b.len() {
            if a[i - 1] == b[j - 1] {
                cur[j] = prev[j - 1] + 1;
                if cur[j] > best_len {
                    best_len = cur[j];
                    best_end = i;
                }
            }
        }
        prev = cur;
    }
    a[best_end - best_len..best_end].to_vec()
}

/// The longest shared segment of every suffix in `sufs` (empty if none is at
/// least [`MIN_SHARED`]). Found from a capped pairwise LCS and then verified
/// against every member, so the result is genuinely common to all.
fn common_substring_all(sufs: &[&[u8]]) -> Vec<u8> {
    if sufs.iter().any(|s| s.is_empty()) {
        return Vec::new();
    }
    let mut cand = lcs_capped(sufs[0], sufs[1], LCS_CAP);
    while !cand.is_empty()
        && !sufs
            .iter()
            .all(|s| s.windows(cand.len()).any(|w| w == cand.as_slice()))
    {
        cand.remove(0);
    }
    if cand.len() < MIN_SHARED {
        Vec::new()
    } else {
        cand
    }
}

/// Split a cohort into shared segments and holes. The shared segments appear in
/// the same order in every member, so `[Hole, Shared, Hole, …]` is a single
/// structure all members share; the holes are the innovation. Deterministic.
fn decompose(members: &[Vec<u8>]) -> Vec<Piece> {
    if members.iter().all(|m| m == &members[0]) {
        if members[0].is_empty() {
            return Vec::new();
        }
        return vec![Piece::Shared(members[0].clone())];
    }
    let k = members.len();
    let mut cur = vec![0usize; k];
    let mut pieces: Vec<Piece> = Vec::new();
    let mut hole = 0usize;
    loop {
        let sufs: Vec<&[u8]> = (0..k).map(|i| &members[i][cur[i]..]).collect();
        let seg = common_substring_all(&sufs);
        if seg.is_empty() {
            break;
        }
        let mut poses = vec![0usize; k];
        let mut region_nonempty = false;
        for i in 0..k {
            let p = find(&members[i][cur[i]..], &seg).unwrap_or(0);
            poses[i] = p;
            if p > 0 {
                region_nonempty = true;
            }
        }
        if region_nonempty {
            pieces.push(Piece::Hole(hole));
            hole += 1;
        }
        pieces.push(Piece::Shared(seg.clone()));
        for i in 0..k {
            cur[i] += poses[i] + seg.len();
        }
    }
    if (0..k).any(|i| cur[i] < members[i].len()) {
        pieces.push(Piece::Hole(hole));
    }
    if pieces.is_empty() {
        pieces.push(Piece::Hole(0));
    }
    pieces
}

/// A hash-consing program builder: identical nodes get one id, so a repeated
/// sub-skeleton is stored once and referenced. `requested` counts every node the
/// naive tree would have emitted; `nodes.len()` is the DAG's actual count.
#[allow(dead_code)]
struct Builder {
    nodes: Vec<Node>,
    literals: Vec<Vec<u8>>,
    lit_index: BTreeMap<Vec<u8>, LiteralId>,
    canon: BTreeMap<Vec<u8>, NodeId>,
    requested: usize,
}

fn key_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

#[allow(dead_code)]
impl Builder {
    fn new() -> Builder {
        Builder {
            nodes: Vec::new(),
            literals: Vec::new(),
            lit_index: BTreeMap::new(),
            canon: BTreeMap::new(),
            requested: 0,
        }
    }

    fn intern(&mut self, op: Op, key: Vec<u8>) -> NodeId {
        self.requested += 1;
        if let Some(&id) = self.canon.get(&key) {
            return id;
        }
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(Node {
            op,
            span: ProgSpan::EMPTY,
        });
        self.canon.insert(key, id);
        id
    }

    fn literal(&mut self, bytes: &[u8]) -> NodeId {
        let lid = match self.lit_index.get(bytes) {
            Some(&id) => id,
            None => {
                let id = LiteralId(self.literals.len() as u32);
                self.literals.push(bytes.to_vec());
                self.lit_index.insert(bytes.to_vec(), id);
                id
            }
        };
        let mut key = vec![0u8];
        key_u32(&mut key, lid.0);
        self.intern(Op::Literal(lid), key)
    }

    fn ref_slot(&mut self, i: u32) -> NodeId {
        let mut key = vec![1u8];
        key_u32(&mut key, i);
        self.intern(
            Op::Ref {
                target: RefTarget::Slot(i),
            },
            key,
        )
    }

    fn ref_material(&mut self, i: u32) -> NodeId {
        let mut key = vec![2u8];
        key_u32(&mut key, i);
        self.intern(
            Op::Ref {
                target: RefTarget::Material(i),
            },
            key,
        )
    }

    fn concat(&mut self, a: NodeId, b: NodeId) -> NodeId {
        let mut key = vec![3u8];
        key_u32(&mut key, a.0);
        key_u32(&mut key, b.0);
        self.intern(Op::Concat { parts: [a, b] }, key)
    }

    fn template(&mut self, program: NodeId, slots: &[NodeId]) -> NodeId {
        let mut key = vec![4u8];
        key_u32(&mut key, program.0);
        key_u32(&mut key, slots.len() as u32);
        for s in slots {
            key_u32(&mut key, s.0);
        }
        self.intern(
            Op::Template {
                program,
                slots: slots.to_vec(),
            },
            key,
        )
    }

    fn finish(self, root: NodeId) -> Program {
        Program {
            nodes: self.nodes,
            literals: self.literals,
            root,
        }
    }
}

#[allow(dead_code)]
fn build_dag(pieces: &[Piece]) -> (Program, usize) {
    let slots = pieces
        .iter()
        .filter(|p| matches!(p, Piece::Hole(_)))
        .count();
    let mut b = Builder::new();
    // Slot values: `Ref{Material(i)}` supplies hole `i` from the caller's refs.
    let mut slot_nodes = Vec::with_capacity(slots);
    for i in 0..slots {
        slot_nodes.push(b.ref_material(i as u32));
    }
    let mut acc: Option<NodeId> = None;
    for p in pieces {
        let id = match p {
            Piece::Shared(seg) => b.literal(seg),
            Piece::Hole(i) => b.ref_slot(*i as u32),
        };
        acc = Some(match acc {
            None => id,
            Some(prev) => b.concat(prev, id),
        });
    }
    let body = acc.unwrap_or_else(|| b.literal(&[]));
    let root = b.template(body, &slot_nodes);
    let requested = b.requested;
    (b.finish(root), requested)
}

/// Anti-unify a cohort into a shared DAG: shared segments stored once, repeated
/// sub-skeletons referenced. The emitted program is valid for
/// `procedural::execute` and its serialized length is the program's cost.
/// `members` must be non-empty.
#[allow(dead_code)]
pub fn synthesise_dag(members: &[Vec<u8>]) -> Dag {
    assert!(!members.is_empty(), "synthesise_dag: empty cohort");
    let pieces = decompose(members);
    let slots = pieces
        .iter()
        .filter(|p| matches!(p, Piece::Hole(_)))
        .count();
    let shared_bytes = pieces
        .iter()
        .map(|p| match p {
            Piece::Shared(s) => s.len(),
            Piece::Hole(_) => 0,
        })
        .sum();
    let (program, naive_nodes) = build_dag(&pieces);
    let program_bytes = crate::procedural::progcodec::best_program_bytes(&program);
    let nodes = program.nodes.len();
    Dag {
        program,
        program_bytes,
        pieces,
        slots,
        nodes,
        naive_nodes,
        shared_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procedural::deserialize;

    #[test]
    fn anti_unifies_a_two_member_cohort_to_the_known_skeleton() {
        let members = vec![b"{{a|1}}".to_vec(), b"{{a|2}}".to_vec()];
        let sk = anti_unify(&members);
        assert_eq!(sk.prefix, b"{{a|".to_vec());
        assert_eq!(sk.suffix, b"}}".to_vec());
        assert_eq!(sk.base(), b"{{a|}}".to_vec());
        // The skeleton with the right hole reproduces each member exactly.
        assert_eq!(materialise_with_hole(&sk, b"1").unwrap(), members[0]);
        assert_eq!(materialise_with_hole(&sk, b"2").unwrap(), members[1]);
        // The empty-hole base is prefix ++ suffix.
        assert_eq!(materialise_base(&sk).unwrap(), b"{{a|}}".to_vec());
    }

    #[test]
    fn the_skeleton_program_is_a_canonical_vm_program() {
        let sk = anti_unify(&[b"<x>one</x>".to_vec(), b"<x>two</x>".to_vec()]);
        assert_eq!(
            deserialize(&serialize(&sk.program)),
            Some(sk.program.clone())
        );
        assert!(
            sk.program_bytes <= serialize(&sk.program).len(),
            "the coded program must never exceed its raw serialization"
        );
        assert!(
            sk.program_bytes > 0,
            "a charged program must cost something"
        );
        assert_eq!(sk.nodes, sk.program.nodes.len());
    }

    #[test]
    fn disjoint_members_share_nothing_and_still_round_trip() {
        // The pure anti-unifier shares nothing here but still round-trips.
        let members = vec![b"aaaa".to_vec(), b"bbbb".to_vec()];
        let sk = anti_unify(&members);
        assert!(sk.prefix.is_empty() && sk.suffix.is_empty());
        assert_eq!(materialise_with_hole(&sk, b"aaaa").unwrap(), members[0]);
        assert_eq!(materialise_with_hole(&sk, b"bbbb").unwrap(), members[1]);
    }

    #[test]
    fn prefix_and_suffix_never_overlap() {
        let members = vec![b"ab".to_vec(), b"ab".to_vec()];
        let sk = anti_unify(&members);
        assert!(sk.prefix.len() + sk.suffix.len() <= 2);
    }

    #[test]
    fn literal_program_round_trips() {
        let sk = literal(b"{{cite|a=1}}");
        assert_eq!(materialise_base(&sk).unwrap(), b"{{cite|a=1}}".to_vec());
        assert!(sk.program_bytes >= b"{{cite|a=1}}".len());
    }

    #[test]
    fn a_singleton_is_not_charged_a_program() {
        // The claim under test: below the minimum, no program is paid at all.
        let one = vec![b"{{cite|url=http://example.org/very/long|title=Example}}".to_vec()];
        assert_eq!(
            synthesise_cohort(&one, DEFAULT_MIN_COHORT),
            Plan::Unshared(UnsharedReason::TooSmall)
        );
        let sk = synthesise_with_min(&one, DEFAULT_MIN_COHORT);
        assert_eq!(sk.program_bytes, 0, "a singleton must not pay a program");

        // Two members are also below the default minimum.
        let two = vec![b"{{a|1}}".to_vec(), b"{{a|2}}".to_vec()];
        assert!(matches!(
            synthesise_cohort(&two, 3),
            Plan::Unshared(UnsharedReason::TooSmall)
        ));
        assert_eq!(synthesise_with_min(&two, 3).program_bytes, 0);

        // And the unshared skeleton still materialises its member directly.
        assert_eq!(
            materialise_with_hole(&sk, one[0].as_slice()).unwrap(),
            one[0]
        );
    }

    #[test]
    fn close_cohorts_share_and_badly_matched_bases_fall_back() {
        let close = vec![
            b"{{cite|a=1}}".to_vec(),
            b"{{cite|a=2}}".to_vec(),
            b"{{cite|a=3}}".to_vec(),
        ];
        match synthesise_cohort(&close, 3) {
            Plan::Shared(sk) => assert!(sk.program_bytes > 0),
            other => panic!("expected a shared plan, got {other:?}"),
        }
        let split = vec![
            b"{{cite|a=1}}".to_vec(),
            b"{{cite|a=2}}".to_vec(),
            b"a completely unrelated member with nothing in common".to_vec(),
        ];
        assert_eq!(
            synthesise_cohort(&split, 3),
            Plan::Unshared(UnsharedReason::BaseNotClose)
        );
    }

    #[test]
    fn the_closeness_test_rejects_a_badly_matched_base() {
        let good_base: &[u8] = b"{{cite|a=}}";
        let good = vec![b"{{cite|a=1}}".to_vec(), b"{{cite|a=2}}".to_vec()];
        assert!(patch_profitable(good_base, &good));
        let bad_base: &[u8] = b"";
        let bad = vec![
            b"{{cite|url=http://example.org/page|title=Example}}".to_vec(),
            b"totally different bytes entirely".to_vec(),
        ];
        assert!(!patch_profitable(bad_base, &bad));
    }

    #[test]
    fn anti_unification_two_member_dag_matches_the_expected_skeleton() {
        let members = vec![b"{{a|1}}".to_vec(), b"{{a|2}}".to_vec()];
        let dag = synthesise_dag(&members);
        assert_eq!(
            dag.pieces,
            vec![
                Piece::Shared(b"{{a|".to_vec()),
                Piece::Hole(0),
                Piece::Shared(b"}}".to_vec()),
            ]
        );
        assert_eq!(dag.slots, 1);
        assert_eq!(dag.materialise(&[b"1".to_vec()]).unwrap(), members[0]);
        assert_eq!(dag.materialise(&[b"2".to_vec()]).unwrap(), members[1]);
        assert!(dag.reconstructs(&members));
        // The DAG is a real program: canonical, deserializable, executable.
        assert_eq!(
            deserialize(&serialize(&dag.program)),
            Some(dag.program.clone())
        );
        assert_eq!(
            dag.program_bytes,
            crate::procedural::progcodec::best_program_bytes(&dag.program)
        );
    }

    #[test]
    fn the_dag_shares_a_repeated_sub_skeleton_with_fewer_nodes() {
        // `ab` is shared three times; the naive tree emits three literal nodes,
        // the DAG stores one and references it.
        let members = vec![b"abXabYab".to_vec(), b"abPabQab".to_vec()];
        let dag = synthesise_dag(&members);
        assert!(dag.reconstructs(&members));
        assert!(
            dag.nodes < dag.naive_nodes,
            "DAG nodes {} should be < naive {}",
            dag.nodes,
            dag.naive_nodes
        );
        eprintln!(
            "[dag] repeated-sub-skeleton: nodes={} naive={} saving={} literals={}",
            dag.nodes,
            dag.naive_nodes,
            dag.naive_nodes - dag.nodes,
            dag.program.literals.len()
        );
        // Every shared segment is stored exactly once in the literal pool.
        let shared: Vec<&Vec<u8>> = dag
            .pieces
            .iter()
            .filter_map(|p| match p {
                Piece::Shared(s) => Some(s),
                Piece::Hole(_) => None,
            })
            .collect();
        assert_eq!(shared.len(), 3);
        let ab = b"ab".to_vec();
        assert_eq!(dag.program.literals.iter().filter(|l| **l == ab).count(), 1);
    }

    #[test]
    fn dag_reconstructs_members_with_multiple_distinct_holes() {
        let members = vec![
            b"{{cite|url=http://a|title=A}}".to_vec(),
            b"{{cite|url=http://bb|title=BB}}".to_vec(),
        ];
        let dag = synthesise_dag(&members);
        // The leading brace text is shared, so the DAG has at least one segment.
        assert!(dag.shared_bytes > 0);
        assert!(dag.reconstructs(&members));
    }

    /// Measures how much DAG sharing (storing a repeated sub-skeleton once)
    /// actually saves on the real cohorts of the development rung. Deliberate:
    ///
    /// ```text
    /// tools/memcap.sh 8 cargo test --release --lib --features procedural \
    ///     -- --ignored --nocapture procedure::skeleton::tests::enwik6_dag_node_savings
    /// ```
    #[test]
    #[ignore = "measures enwik6; run deliberately"]
    fn enwik6_dag_node_savings() {
        use crate::procedure::cohort;
        use crate::procedure::extract::{self, ClassName};
        let data = std::fs::read("evidence/corpus/enwik6").expect("enwik6");
        // The generic-shape classes collapse to one huge cohort, where DAG
        // synthesis on short members is not the question; measure the named ones.
        for class in [
            ClassName::Template,
            ClassName::WikiLink,
            ClassName::WikiTable,
            ClassName::XmlOpen,
        ] {
            let spans = extract::extract(&data, class, 1000, 1, 1 << 20).spans;
            let cohorts = cohort::discover(&data, &spans, class);
            let (mut nodes, mut naive, mut repeated, mut considered) =
                (0usize, 0usize, 0usize, 0usize);
            for c in &cohorts {
                // Cap the members considered, deterministically, so a very large
                // cohort cannot dominate the measurement's runtime.
                let sample: Vec<Vec<u8>> = c
                    .members
                    .iter()
                    .take(16)
                    .map(|&i| data[spans[i].start..spans[i].end()].to_vec())
                    .collect();
                if sample.len() < 2 {
                    continue;
                }
                let dag = synthesise_dag(&sample);
                considered += 1;
                nodes += dag.nodes;
                naive += dag.naive_nodes;
                if dag.naive_nodes > dag.nodes {
                    repeated += 1;
                }
            }
            eprintln!(
                "[dag] {:11} cohorts>=2: {:4} nodes={:5} naive={:5} saving={:5} with_repetition={:4}",
                class.name(),
                considered,
                nodes,
                naive,
                naive.saturating_sub(nodes),
                repeated
            );
        }
    }
}
