//! Phase 14.32: the PPM family, split by lifetime.
//!
//! The project already has one bounded PPM-C expert; it lives in
//! `crate::context` because a context-mixing roster consumes it as one more
//! expert. This module is for the *distribution* form of the same family —
//! an expert that exposes an entire next-byte distribution rather than being
//! reduced to a single bit probability before anything can use it.
//!
//! **Gated on `phase14`.** These are screening tools: they exist to be measured
//! against the incumbent at equal memory before anything is wired into the
//! pipeline, and nothing in the accepted configuration reads them yet.

#[cfg(feature = "phase14")]
pub mod deep;
