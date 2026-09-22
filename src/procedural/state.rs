//! Phase 14.7: state coding — turning a program's configuration into bytes.
//!
//! **PLACEHOLDER (scaffolding for the P14.7 workstream).** A rank is not
//! automatically the best representation, and this module exists to make that a
//! measurement rather than an assumption.
//!
//! For each program `P` there is a finite state model `Θ(P)` — the slots a
//! `Template` may vary, the subsets a `Choice` may take, the counts a `Repeat`
//! may use. The question this module answers, per program and per slot, is which
//! of these is smallest *in complete bytes*:
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
//! * **A rank is not automatically best.** It wins when
//!   `ceil(log2 |Θ(P)|)` is cheaper than serializing the state variables
//!   independently, and loses when the state is skewed — which is what the rANS
//!   candidate is for.
//! * **The comparison is on serialized bytes**, never on bits-estimates, and the
//!   model's own description is charged to the stream that uses it.

/// PLACEHOLDER: P14.7 lands the state model, the candidate codecs and the
/// per-slot measured competition here. The constant marks the seam so the module
/// is a compilation unit rather than only prose.
pub const P14_7_PLACEHOLDER: bool = true;
