//! Phase 14.5-14.6: target-directed search, bounded, with the real cost as judge.
//!
//! **PLACEHOLDER (scaffolding for the P14.5/P14.6 workstream).**
//!
//! The search runs *backward from the exact target*, never forward from a random
//! program, because a forward search spends almost all of its budget on programs
//! that could not reconstruct the target at all. Each operator supplies an exact
//! decomposer:
//!
//! ```text
//! Repeat    prove target = repeated(child)
//! Slice     prove target = an exact source slice
//! Ref       prove target = existing material
//! Template  derive the exact slot values
//! Concat    propose meaningful target partitions
//! Patch     derive the exact mismatch representation
//! ```
//!
//! Every completed candidate still goes through an independent materialization
//! equality test — the decomposer's proof is an optimisation, not the authority.
//!
//! The search is `f = g + h` with `Literal(target)` as the incumbent from the
//! first step, pruning when `g + h >= incumbent`. Two rules from the plan are
//! non-negotiable:
//!
//! * **selection uses actual complete serialized bytes** — never AST node counts,
//!   coverage estimates, grammar size alone, or a heuristic score;
//! * **a search that saves 200 bytes but costs hours of judged compression work is
//!   not automatically valuable** — so the module records candidate count, search
//!   time, best gain, gain per search-second and gain per search-byte-of-code.

/// PLACEHOLDER: P14.5/P14.6 land the decomposers, the A*/branch-and-bound driver
/// and the measured-cost incumbent here.
pub const P14_5_6_PLACEHOLDER: bool = true;
