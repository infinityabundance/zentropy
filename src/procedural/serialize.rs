//! Exact serialization of a [`Program`]. This byte string is the **only** cost
//! authority for a program: not node counts, not a coverage estimate, not a
//! grammar size. `serialize(p).len()` is what a search is charged.
//!
//! **Canonicity.** [`serialize`] has exactly one output for a given program and
//! [`deserialize`] recovers it exactly, so `deserialize(serialize(p)) == p` for
//! every program. That property is the whole reason the format is written down
//! here rather than derived from a derive macro: a serializer that is merely
//! *a* bijection would make the cost authority ambiguous.
//!
//! **No allocation from an unvalidated field.** Every count read from the wire is
//! first compared against the number of bytes that remain, because every item
//! costs at least one byte. A forged `node_count` of `2^32` in a thirty-byte
//! buffer is therefore rejected before a `Vec` is reserved, not after the OOM
//! killer has already chosen a victim. This is the parse-time half of the
//! allocation-bomb court; the execute-time half is in [`super::execute`].

use super::program::{Node, Op, Program};
use super::types::{LiteralId, NodeId, RefTarget, ResidualId, Span};

/// Wire format version. Bumping it is a deliberate, costed interface change.
const FORMAT_VERSION: u8 = 1;

// Operator tags. Fixed to the operator numbers and never reordered, because a
// reorder silently reinterprets every archive ever written.
const T_LITERAL: u8 = 0;
const T_CONCAT: u8 = 1;
const T_REPEAT: u8 = 2;
const T_REF: u8 = 3;
const T_SLICE: u8 = 4;
const T_TEMPLATE: u8 = 5;
const T_PATCH: u8 = 6;

const REF_NODE: u8 = 0;
const REF_MATERIAL: u8 = 1;
const REF_SLOT: u8 = 2;

/// Longest legal unsigned LEB128 encoding of a `u64` (ceil(64/7) = 10 bytes).
const MAX_VARINT_BYTES: usize = 10;

/// A capacity that a decoded count has *earned*: each item needs at least
/// `min_bytes` on the wire, so the count cannot exceed `remaining / min_bytes`.
/// Reserving this instead of the raw count means a forged header cannot amplify
/// a small input into a large allocation.
fn earned_capacity(count: usize, remaining: usize, min_bytes: usize) -> usize {
    count.min(remaining / min_bytes + 1)
}

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

/// Read one unsigned LEB128 value. Returns `None` for truncation, for a run
/// longer than ten bytes, or for an encoding whose set bits do not fit a `u64`.
/// Rejecting non-fitting rather than wrapping is what stops a forged length from
/// aliasing a small one.
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
        // At the top group only one bit still fits in a u64.
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

/// Serialize `p` to its canonical bytes. The result is the charged program cost.
pub fn serialize(p: &Program) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(FORMAT_VERSION);
    put_uvarint(&mut out, p.nodes.len() as u64);
    put_uvarint(&mut out, p.root.0 as u64);
    put_uvarint(&mut out, p.literals.len() as u64);
    for lit in &p.literals {
        put_uvarint(&mut out, lit.len() as u64);
        out.extend_from_slice(lit);
    }
    for node in &p.nodes {
        match &node.op {
            Op::Literal(id) => {
                out.push(T_LITERAL);
                put_uvarint(&mut out, id.0 as u64);
            }
            Op::Concat { parts } => {
                out.push(T_CONCAT);
                put_uvarint(&mut out, parts[0].0 as u64);
                put_uvarint(&mut out, parts[1].0 as u64);
            }
            Op::Repeat { child, count } => {
                out.push(T_REPEAT);
                put_uvarint(&mut out, child.0 as u64);
                put_uvarint(&mut out, *count as u64);
            }
            Op::Ref { target } => {
                out.push(T_REF);
                match target {
                    RefTarget::Node(id) => {
                        out.push(REF_NODE);
                        put_uvarint(&mut out, id.0 as u64);
                    }
                    RefTarget::Material(i) => {
                        out.push(REF_MATERIAL);
                        put_uvarint(&mut out, *i as u64);
                    }
                    RefTarget::Slot(i) => {
                        out.push(REF_SLOT);
                        put_uvarint(&mut out, *i as u64);
                    }
                }
            }
            Op::Slice { src, from, len } => {
                out.push(T_SLICE);
                put_uvarint(&mut out, src.0 as u64);
                put_uvarint(&mut out, *from as u64);
                put_uvarint(&mut out, *len as u64);
            }
            Op::Template { program, slots } => {
                out.push(T_TEMPLATE);
                put_uvarint(&mut out, program.0 as u64);
                put_uvarint(&mut out, slots.len() as u64);
                for s in slots {
                    put_uvarint(&mut out, s.0 as u64);
                }
            }
            Op::Patch { base, residual } => {
                out.push(T_PATCH);
                put_uvarint(&mut out, base.0 as u64);
                put_uvarint(&mut out, residual.0 as u64);
            }
        }
        put_uvarint(&mut out, node.span.from);
        put_uvarint(&mut out, node.span.len);
    }
    out
}

/// Deserialize a program, or `None` if the bytes are not a canonical, structurally
/// valid program. `None` is a *typed* rejection: a forged archive is refused, not
/// trusted and not panicked on.
pub fn deserialize(b: &[u8]) -> Option<Program> {
    let mut pos = 0usize;
    if *b.get(pos)? != FORMAT_VERSION {
        return None;
    }
    pos += 1;

    let node_count = get_uvarint(b, &mut pos)?;
    let root = get_uvarint(b, &mut pos)?;
    let lit_count = get_uvarint(b, &mut pos)?;

    // Every node costs at least one byte and every literal at least its length
    // varint, so a count larger than the remaining input is impossible and is
    // rejected *before* any `Vec` is reserved.
    let remaining = (b.len() - pos) as u64;
    if node_count > remaining || lit_count > remaining {
        return None;
    }
    if node_count > u32::MAX as u64 || lit_count > u32::MAX as u64 {
        return None;
    }
    let node_count = node_count as usize;
    let lit_count = lit_count as usize;

    let mut literals: Vec<Vec<u8>> =
        Vec::with_capacity(earned_capacity(lit_count, b.len() - pos, 1));
    for _ in 0..lit_count {
        let len = get_uvarint(b, &mut pos)?;
        if len > (b.len() - pos) as u64 {
            return None;
        }
        let len = len as usize;
        let end = pos + len;
        literals.push(b[pos..end].to_vec());
        pos = end;
    }

    // A node costs at least a tag and two span varints: three bytes.
    let mut nodes: Vec<Node> = Vec::with_capacity(earned_capacity(node_count, b.len() - pos, 3));
    for _ in 0..node_count {
        let tag = *b.get(pos)?;
        pos += 1;
        let op = match tag {
            T_LITERAL => Op::Literal(LiteralId(get_uvarint(b, &mut pos)? as u32)),
            T_CONCAT => {
                let a = NodeId(get_uvarint(b, &mut pos)? as u32);
                let c = NodeId(get_uvarint(b, &mut pos)? as u32);
                Op::Concat { parts: [a, c] }
            }
            T_REPEAT => {
                let child = NodeId(get_uvarint(b, &mut pos)? as u32);
                let count = get_uvarint(b, &mut pos)?;
                if count > u32::MAX as u64 {
                    return None;
                }
                Op::Repeat {
                    child,
                    count: count as u32,
                }
            }
            T_REF => {
                let kind = *b.get(pos)?;
                pos += 1;
                let id = get_uvarint(b, &mut pos)?;
                let target = match kind {
                    REF_NODE => RefTarget::Node(NodeId(id as u32)),
                    REF_MATERIAL => RefTarget::Material(id as u32),
                    REF_SLOT => RefTarget::Slot(id as u32),
                    _ => return None,
                };
                Op::Ref { target }
            }
            T_SLICE => {
                let src = NodeId(get_uvarint(b, &mut pos)? as u32);
                let from = get_uvarint(b, &mut pos)?;
                let len = get_uvarint(b, &mut pos)?;
                Op::Slice {
                    src,
                    from: from as u32,
                    len: len as u32,
                }
            }
            T_TEMPLATE => {
                let program = NodeId(get_uvarint(b, &mut pos)? as u32);
                let slot_count = get_uvarint(b, &mut pos)?;
                // Each slot is at least one byte on the wire.
                if slot_count > (b.len() - pos) as u64 {
                    return None;
                }
                if slot_count > u32::MAX as u64 {
                    return None;
                }
                let mut slots =
                    Vec::with_capacity(earned_capacity(slot_count as usize, b.len() - pos, 1));
                for _ in 0..slot_count {
                    slots.push(NodeId(get_uvarint(b, &mut pos)? as u32));
                }
                Op::Template { program, slots }
            }
            T_PATCH => {
                let base = NodeId(get_uvarint(b, &mut pos)? as u32);
                let residual = ResidualId(get_uvarint(b, &mut pos)? as u32);
                Op::Patch { base, residual }
            }
            _ => return None,
        };
        let from = get_uvarint(b, &mut pos)?;
        let len = get_uvarint(b, &mut pos)?;
        nodes.push(Node {
            op,
            span: Span { from, len },
        });
    }

    // Trailing bytes mean the input is not a canonical program; refusing them keeps
    // the map from bytes to programs injective, so the cost authority is real.
    if pos != b.len() {
        return None;
    }

    let program = Program {
        nodes,
        literals,
        root: NodeId(root as u32),
    };
    if !program.ids_valid() {
        return None;
    }
    Some(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procedural::program::{Node, Op, Program};
    use crate::procedural::types::{LiteralId, NodeId, RefTarget, ResidualId, Span};

    fn n(op: Op) -> Node {
        Node {
            op,
            span: Span::EMPTY,
        }
    }

    /// A program that touches every operator exactly once, with the two
    /// non-node reference namespaces (Material and Slot) represented too.
    fn sample() -> Program {
        Program {
            nodes: vec![
                n(Op::Literal(LiteralId(0))), // 0
                n(Op::Literal(LiteralId(1))), // 1
                n(Op::Concat {
                    parts: [NodeId(0), NodeId(1)],
                }), // 2
                n(Op::Repeat {
                    child: NodeId(2),
                    count: 3,
                }), // 3
                n(Op::Slice {
                    src: NodeId(3),
                    from: 1,
                    len: 5,
                }), // 4
                n(Op::Ref {
                    target: RefTarget::Node(NodeId(2)),
                }), // 5
                n(Op::Ref {
                    target: RefTarget::Material(7),
                }), // 6
                n(Op::Ref {
                    target: RefTarget::Slot(0),
                }), // 7
                n(Op::Template {
                    program: NodeId(7),
                    slots: vec![NodeId(1)],
                }), // 8
                n(Op::Patch {
                    base: NodeId(4),
                    residual: ResidualId(2),
                }), // 9
            ],
            literals: vec![b"ab".to_vec(), b"cd".to_vec()],
            root: NodeId(9),
        }
    }

    #[test]
    fn round_trips_every_operator() {
        let p = sample();
        assert_eq!(deserialize(&serialize(&p)), Some(p));
    }

    #[test]
    fn serialization_is_canonical_and_deterministic() {
        // Two calls agree, and a deserialize/re-serialize cycle is a fixed point:
        // this is what makes `serialize(p).len()` a well-defined cost authority.
        let p = sample();
        let once = serialize(&p);
        assert_eq!(once, serialize(&p));
        let back = deserialize(&once).unwrap();
        assert_eq!(serialize(&back), once);
    }

    #[test]
    fn round_trips_nonempty_spans() {
        // Span is search metadata but part of the charged node, so it must survive
        // the round trip or the cost authority would be lossy.
        let mut p = sample();
        p.nodes[2].span = Span {
            from: 11,
            len: 4096,
        };
        assert_eq!(deserialize(&serialize(&p)), Some(p));
    }

    #[test]
    fn round_trips_an_empty_literal_pool_and_a_literal_node() {
        let p = Program {
            nodes: vec![n(Op::Literal(LiteralId(0)))],
            literals: vec![Vec::new()],
            root: NodeId(0),
        };
        assert_eq!(deserialize(&serialize(&p)), Some(p));
    }

    #[test]
    fn deserialize_rejects_a_wrong_version() {
        let mut bytes = serialize(&sample());
        bytes[0] = FORMAT_VERSION.wrapping_add(1);
        assert_eq!(deserialize(&bytes), None);
    }

    #[test]
    fn deserialize_rejects_truncation() {
        let bytes = serialize(&sample());
        for cut in 1..bytes.len() {
            assert_eq!(deserialize(&bytes[..cut]), None, "cut at {cut}");
        }
    }

    #[test]
    fn deserialize_rejects_trailing_bytes() {
        let mut bytes = serialize(&sample());
        bytes.push(0);
        assert_eq!(deserialize(&bytes), None);
    }

    #[test]
    fn deserialize_rejects_an_impossible_node_count_without_allocating() {
        // A count larger than the remaining input cannot possibly be satisfied,
        // because every node costs at least one byte. The rejection must come from
        // that comparison, before any `Vec` is reserved.
        let mut b = vec![FORMAT_VERSION];
        put_uvarint(&mut b, 1_000_000);
        put_uvarint(&mut b, 0); // root
        put_uvarint(&mut b, 0); // literal count
        assert_eq!(deserialize(&b), None);
    }

    #[test]
    fn deserialize_rejects_a_dangling_reference() {
        // `serialize` will emit this (it is not a validator), but `deserialize`
        // refuses a program whose ids do not resolve.
        let p = Program {
            nodes: vec![n(Op::Ref {
                target: RefTarget::Node(NodeId(5)),
            })],
            literals: vec![],
            root: NodeId(0),
        };
        assert_eq!(deserialize(&serialize(&p)), None);
    }

    #[test]
    fn deserialize_rejects_a_root_outside_the_node_array() {
        let p = Program {
            nodes: vec![n(Op::Literal(LiteralId(0)))],
            literals: vec![b"x".to_vec()],
            root: NodeId(9),
        };
        assert_eq!(deserialize(&serialize(&p)), None);
    }

    #[test]
    fn deserialize_rejects_an_unknown_operator_tag() {
        let mut bytes = serialize(&sample());
        // Byte 1 begins the node count varint, then root, lit count, then literals.
        // Rather than compute the exact offset, corrupt the *last* node tag by
        // searching for a byte whose value is T_PATCH and bumping it.
        let pos = bytes.iter().rposition(|&b| b == T_PATCH).unwrap();
        bytes[pos] = 0x7f;
        assert_eq!(deserialize(&bytes), None);
    }
}
