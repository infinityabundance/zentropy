//! The seven-operator program: [`Op`], [`Node`], [`Program`].
//!
//! The operator basis is deliberately small (plan §4.2). Synthesis quality and
//! the state coding matter more than the size of the language, and every operator
//! added has to demonstrate a measurable codelength opportunity, so there are
//! exactly seven and no more.
//!
//! **Why `Concat` is binary.** The plan sketches a bounded-arity `parts` vector;
//! the arity table in the module root fixes it at 2, and a fixed `[NodeId; 2]`
//! makes the shape of the tree a type-level fact. A variable arity would let a
//! forged archive claim an enormous arity and force a large allocation before any
//! bound could be checked. A left-deep binary tree costs one node per extra part,
//! which is a real and measurable price a search can weigh against a `Literal`.
//!
//! **Why literals live in a pool.** Two nodes that need the same bytes should pay
//! for them once; the pool makes that sharing explicit and keeps a node's serial
//! form a fixed width rather than proportional to its literal. Residuals are *not*
//! in the program: they are the innovation, charged in their own stream (plan
//! §4.4), and only referenced here.

use super::types::{LiteralId, NodeId, RefTarget, ResidualId, Span};
use super::OpKind;

/// One operation of the procedural DSL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// The universal fallback: raw bytes from the literal pool. Always available,
    /// so a program is never unable to describe an arbitrary target.
    Literal(LiteralId),
    /// Ordered concatenation of exactly two children, left then right.
    Concat { parts: [NodeId; 2] },
    /// `child` materialized exactly `count` times in sequence.
    Repeat { child: NodeId, count: u32 },
    /// A reference to already-materialised material (node output, caller-supplied
    /// bytes, or a template slot); see [`RefTarget`].
    Ref { target: RefTarget },
    /// A bounded range `[from, from + len)` of `src`'s output.
    Slice { src: NodeId, from: u32, len: u32 },
    /// A shared skeleton `program` with typed `slots` substituted at the skeleton's
    /// `RefTarget::Slot` sites. This is what lets many targets share one shape
    /// while differing only in their holes.
    Template { program: NodeId, slots: Vec<NodeId> },
    /// `base` with a typed residual applied; the residual is referenced into the
    /// caller's residual pool rather than inlined.
    Patch { base: NodeId, residual: ResidualId },
}

impl Op {
    /// The operator class, for attribution and for the arity table.
    pub fn kind(&self) -> OpKind {
        match self {
            Op::Literal(_) => OpKind::Literal,
            Op::Concat { .. } => OpKind::Concat,
            Op::Repeat { .. } => OpKind::Repeat,
            Op::Ref { .. } => OpKind::Ref,
            Op::Slice { .. } => OpKind::Slice,
            Op::Template { .. } => OpKind::Template,
            Op::Patch { .. } => OpKind::Patch,
        }
    }
}

/// A node: an operator plus the target extent it is meant to cover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub op: Op,
    pub span: Span,
}

/// A whole procedural program. `root` is the node whose output is the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub nodes: Vec<Node>,
    /// The literal pool, indexed by [`LiteralId`].
    pub literals: Vec<Vec<u8>>,
    pub root: NodeId,
}

impl Program {
    /// The node at `id`, or `None` if the id is outside the program.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0 as usize)
    }

    /// The literal at `id`, or `None` if the id is outside the pool.
    pub fn literal(&self, id: LiteralId) -> Option<&[u8]> {
        self.literals.get(id.0 as usize).map(|v| v.as_slice())
    }

    /// Structural well-formedness, independent of any [`super::Bounds`]: every id a
    /// node refers to must name something that exists. Serialization cannot produce
    /// a program that fails this, but a program built by hand (tests, search) can,
    /// and checking it separately keeps the "malformed" and "over-budget"
    /// rejections from being confused with each other.
    pub fn ids_valid(&self) -> bool {
        let n = self.nodes.len() as u64;
        let l = self.literals.len() as u64;
        if self.root.0 as u64 >= n {
            return false;
        }
        let node_ok = |id: NodeId| (id.0 as u64) < n;
        let lit_ok = |id: LiteralId| (id.0 as u64) < l;
        self.nodes.iter().all(|node| match &node.op {
            Op::Literal(id) => lit_ok(*id),
            Op::Concat { parts } => node_ok(parts[0]) && node_ok(parts[1]),
            Op::Repeat { child, .. } => node_ok(*child),
            Op::Ref { target } => match target {
                RefTarget::Node(id) => node_ok(*id),
                // Material and Slot ids name tables that live outside the program,
                // so they are checked at execute time, not here.
                RefTarget::Material(_) | RefTarget::Slot(_) => true,
            },
            Op::Slice { src, .. } => node_ok(*src),
            Op::Template { program, slots } => {
                node_ok(*program) && slots.iter().all(|s| node_ok(*s))
            }
            Op::Patch { base, .. } => node_ok(*base),
        })
    }
}
