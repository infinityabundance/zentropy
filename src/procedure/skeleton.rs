//! Skeleton synthesis / anti-unification (§14.14).
//!
//! For one cohort we build **one** shared candidate from all its members. The
//! skeleton is an anti-unifier:
//!
//! * a byte-level **common prefix** shared by every member;
//! * a byte-level **common suffix** shared by every member (non-overlapping with
//!   the prefix);
//! * a **single differing region** — the structural "slot" — between them.
//!
//! The slot is what makes the skeleton a *skeleton*: the shared parts are literal
//! bytes (charged once, to the program), and each member supplies only its own
//! hole as state/residual.
//!
//! **Why one slot, and why byte-level.** Every target this experiment extracts is
//! a *single* IR token (`{{...}}`, `<tag ...>`, `[[...]]`, a URL, a number, an
//! entity), so its [`crate::ir::Kind`] sequence has length one. There is therefore
//! no finer kind structure inside a target to align to: the only structure the
//! members of a cohort share is their bytes, and the anti-unifier is a byte-level
//! common prefix/suffix with exactly one differing interior region. The Kind
//! sequence still does its job where it is informative — as the cohort signature
//! ([`super::cohort::shape`]) for classes that have no name signature.
//!
//! The candidate is emitted as a real [`Program`] using the real VM operators —
//! `Template` with a `Ref{Slot(0)}` hole whose value node is
//! `Ref{Material(0)}` — and its only cost authority is
//! [`serialize`]`(program).len()`. `execute` with `refs == [hole]` must
//! reproduce the member exactly, which is asserted by the caller.
//!
//! Two other bases are supported, for the §14.44 controls:
//!
//! * [`literal`] — a one-node program that materialises a byte string verbatim.
//!   Used per-member for the `Literal`-only control (no sharing at all) and once
//!   for the best-member-prototype control.

use crate::procedural::execute::{execute, ExecContext};
use crate::procedural::program::{Node, Op, Program};
use crate::procedural::serialize::serialize;
use crate::procedural::types::{LiteralId, NodeId, RefTarget, Span as ProgSpan};
use crate::procedural::Bounds;

/// A synthesised skeleton, plus the byte strings needed to derive member
/// residuals against its empty-hole base.
#[derive(Clone, Debug)]
pub struct Skeleton {
    pub program: Program,
    /// `serialize(program).len()` — the only program cost authority.
    pub program_bytes: usize,
    /// Bytes shared by every member at the front.
    pub prefix: Vec<u8>,
    /// Bytes shared by every member at the back.
    pub suffix: Vec<u8>,
}

impl Skeleton {
    /// The base this skeleton materialises with an empty hole: `prefix ++ suffix`.
    pub fn base(&self) -> Vec<u8> {
        let mut v = self.prefix.clone();
        v.extend_from_slice(&self.suffix);
        v
    }
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

/// Anti-unify a cohort into one shared skeleton. `members` must be non-empty.
pub fn synthesise(members: &[Vec<u8>]) -> Skeleton {
    assert!(!members.is_empty(), "synthesise: empty cohort");
    let prefix = common_prefix(members);
    let suffix = common_suffix(members, prefix.len());
    let program = hole_template(&prefix, &suffix);
    let program_bytes = serialize(&program).len();
    Skeleton {
        program,
        program_bytes,
        prefix,
        suffix,
    }
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
    let program_bytes = serialize(&program).len();
    Skeleton {
        program,
        program_bytes,
        prefix: bytes.to_vec(),
        suffix: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procedural::deserialize;

    #[test]
    fn anti_unifies_a_two_member_cohort_to_the_known_skeleton() {
        let members = vec![b"{{a|1}}".to_vec(), b"{{a|2}}".to_vec()];
        let sk = synthesise(&members);
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
        let sk = synthesise(&[b"<x>one</x>".to_vec(), b"<x>two</x>".to_vec()]);
        assert_eq!(
            deserialize(&serialize(&sk.program)),
            Some(sk.program.clone())
        );
        assert_eq!(sk.program_bytes, serialize(&sk.program).len());
    }

    #[test]
    fn disjoint_members_share_nothing_and_still_round_trip() {
        let members = vec![b"aaaa".to_vec(), b"bbbb".to_vec()];
        let sk = synthesise(&members);
        assert!(sk.prefix.is_empty() && sk.suffix.is_empty());
        assert_eq!(materialise_with_hole(&sk, b"aaaa").unwrap(), members[0]);
        assert_eq!(materialise_with_hole(&sk, b"bbbb").unwrap(), members[1]);
    }

    #[test]
    fn prefix_and_suffix_never_overlap() {
        let members = vec![b"ab".to_vec(), b"ab".to_vec()];
        let sk = synthesise(&members);
        assert!(sk.prefix.len() + sk.suffix.len() <= 2);
    }

    #[test]
    fn literal_program_round_trips() {
        let sk = literal(b"{{cite|a=1}}");
        assert_eq!(materialise_base(&sk).unwrap(), b"{{cite|a=1}}".to_vec());
        assert!(sk.program_bytes >= b"{{cite|a=1}}".len());
    }
}
