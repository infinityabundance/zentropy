//! Identifier and geometry types shared by the procedural VM.
//!
//! **Why newtypes.** Every id here indexes one of four different tables — program
//! nodes, the literal pool, the residual pool and the caller's external material —
//! and all four are a bare `u32` on the wire. A plain `u32` would let a call site
//! transpose a literal index into a node index, and the decoder would then obey
//! the transposition silently. The newtypes turn that class of mistake into a
//! compile error, which is the cheapest possible decoder-safety court.
//!
//! **Why `RefTarget` is an enum.** A reference can name either
//! * another node of the *same* program (a DAG edge, which is what makes a shared
//!   program valuable and what makes cycle detection necessary), or
//! * material the caller already materialised (`ExecContext::refs`), or
//! * one of the current [`super::program::Op::Template`]'s slots.
//!
//! Those are three genuinely different namespaces. Collapsing them into one
//! integer would force offset arithmetic at every use site and make a forged
//! index resolve to the wrong namespace instead of a typed rejection.

/// An index into [`super::program::Program::nodes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// An index into [`super::program::Program::literals`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LiteralId(pub u32);

/// An index into [`super::execute::ExecContext::residuals`]. Residuals are the
/// *innovation*, coded separately from the program, so they are not part of the
/// serialized program and are referenced rather than inlined.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResidualId(pub u32);

/// What a reference names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefTarget {
    /// The output of another node of this program.
    Node(NodeId),
    /// Bytes the caller already materialised, by index into
    /// [`super::execute::ExecContext::refs`].
    Material(u32),
    /// The `i`-th slot of the immediately enclosing [`super::program::Op::Template`].
    /// Slots are not inherited across a nested template, so a slot index is never
    /// ambiguous about which template it belongs to.
    Slot(u32),
}

/// The target extent a node is intended to cover.
///
/// Search metadata: it tells the emitter which bytes of the target a node was
/// built from. It has **no effect on materialization** — the decoder never reads
/// it — but it is part of [`super::program::Node`], and the cost authority is the
/// serialized program, so it is charged honestly rather than hidden. A search that
/// cannot pay for its own bookkeeping bytes should not record them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    /// First target byte covered.
    pub from: u64,
    /// Number of target bytes covered.
    pub len: u64,
}

impl Span {
    /// The empty span, the value a node carries when its extent is unknown.
    pub const EMPTY: Span = Span { from: 0, len: 0 };

    /// One-past-the-last target byte covered, saturating rather than wrapping so a
    /// forged span can never wrap into a plausible-looking small extent.
    pub fn end(self) -> u64 {
        self.from.saturating_add(self.len)
    }
}
