//! Phase 14.4-14.8: the bounded procedural DSL.
//!
//! **PLACEHOLDER (P14.0 scaffolding).** This module marks the seam; P14.4-P14.8
//! land the VM, the exact serializer, the bounded decoder, the rank/unrank state
//! coders and the typed residual algebra. The frozen signatures are in
//! [`PHASE14_PLAN.md`](../../docs/PHASE14_PLAN.md) §4.2-§4.4.
//!
//! Non-negotiable properties, recorded here because they are the reason the
//! module exists at all:
//!
//! * **exactness** — `execute(program) ⊕ residual == target`, byte for byte, or a
//!   typed error. Never a partial materialization;
//! * **bounds are enforced, not asserted** — depth, node count, output length,
//!   work and reference count each have an independent ceiling, and a violation
//!   is a typed error rather than a panic, an OOM or a hang;
//! * **cost authority is the serialized bytes** — `serialize(program).len()`, not
//!   an AST node count, not a coverage estimate, not a grammar size.

/// What went wrong when materializing a program. Every variant is a *typed
/// rejection*, so a forged archive is rejected rather than trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecError {
    /// Nesting exceeded `Bounds::max_depth`.
    Depth,
    /// More nodes than `Bounds::max_nodes`.
    Nodes,
    /// Output would exceed `Bounds::max_output`.
    Output,
    /// Work would exceed `Bounds::max_work`.
    Work,
    /// A reference chain revisited a node.
    RefCycle,
    /// A reference or slice pointed outside the material it may read.
    BadRef,
    /// A rank was not a legal coordinate for its state space.
    BadRank,
    /// The program is syntactically invalid.
    Malformed,
}

impl ExecError {
    pub fn name(self) -> &'static str {
        match self {
            ExecError::Depth => "depth",
            ExecError::Nodes => "nodes",
            ExecError::Output => "output",
            ExecError::Work => "work",
            ExecError::RefCycle => "ref_cycle",
            ExecError::BadRef => "bad_ref",
            ExecError::BadRank => "bad_rank",
            ExecError::Malformed => "malformed",
        }
    }
}

/// Independent ceilings. A decoder that enforces five bounds separately cannot
/// be made to panic, OOM or hang by one clever field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub max_depth: u8,
    pub max_nodes: u32,
    pub max_output: u64,
    pub max_work: u64,
    pub max_refs: u32,
}

impl Default for Bounds {
    fn default() -> Self {
        // Conservative defaults; a caller that has measured its own needs raises
        // them explicitly, so the safe value is the one you get by default.
        Bounds {
            max_depth: 16,
            max_nodes: 1 << 20,
            max_output: 1 << 30,
            max_work: 1 << 32,
            max_refs: 1 << 16,
        }
    }
}

/// The operator basis. Deliberately small: the quality of synthesis and of state
/// coding matters more than the size of the language, and every operator added
/// must demonstrate a measurable codelength opportunity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    /// Universal fallback: any bytes, always available.
    Literal,
    /// Ordered concatenation.
    Concat,
    /// Exact bounded repetition.
    Repeat,
    /// Reference already-decoded material.
    Ref,
    /// A bounded range of an existing node's output.
    Slice,
    /// Shared skeleton plus typed slots.
    Template,
    /// Exact residual applied to a base.
    Patch,
}

impl OpKind {
    pub const ALL: [OpKind; 7] = [
        OpKind::Literal,
        OpKind::Concat,
        OpKind::Repeat,
        OpKind::Ref,
        OpKind::Slice,
        OpKind::Template,
        OpKind::Patch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            OpKind::Literal => "literal",
            OpKind::Concat => "concat",
            OpKind::Repeat => "repeat",
            OpKind::Ref => "ref",
            OpKind::Slice => "slice",
            OpKind::Template => "template",
            OpKind::Patch => "patch",
        }
    }

    /// The arity the operator needs. Fixed rather than variable, so a malformed
    /// archive cannot claim a huge arity and force a huge allocation.
    pub fn arity(self) -> usize {
        match self {
            OpKind::Literal => 0,
            OpKind::Concat => 2,
            OpKind::Repeat => 1,
            OpKind::Ref => 0,
            OpKind::Slice => 1,
            OpKind::Template => 1,
            OpKind::Patch => 1,
        }
    }
}

/// PLACEHOLDER: kept as a seam marker so the module's arrival is visible in the
/// compiled artifact; it costs nothing and disappears when the coordinator drops
/// it. The VM itself has landed (see the submodules below).
pub const P14_4_PLACEHOLDER: bool = true;

// Phase 14.4-14.8 deliverables, split so each has one job:
//
// * `types`    — ids and geometry, so an index cannot be transposed;
// * `program`  — the seven operators and the node/program containers;
// * `serialize`— the exact, canonical, allocation-safe byte format (the cost
//                authority);
// * `execute`  — the bounded, cycle-safe materializer;
// * `rank`     — rank/unrank primitives for state coding (§4.3);
// * `residual` — the typed residual algebra (§4.4).
pub mod execute;
pub mod progcodec;
pub mod program;
pub mod rank;
pub mod residual;
pub mod search;
pub mod serialize;
pub mod state;
pub mod types;

pub use execute::{execute, ExecContext};
pub use program::{Node, Op, Program};
pub use residual::{apply, derive, EditOp, Residual, ResidualKind};
pub use serialize::{deserialize, serialize};
pub use types::{LiteralId, NodeId, RefTarget, ResidualId, Span};
