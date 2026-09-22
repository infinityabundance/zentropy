//! # Zentropy
//!
//! Find the smallest executable explanation of `enwik9`. Persist the
//! explanation, encode only the irreducible innovation, and charge every
//! explanatory mechanism for every byte required to reconstruct the exact
//! original.
//!
//! ## The one objective
//!
//! ```text
//! S = submitted_compressor_bytes + self_extracting_archive_bytes
//! ```
//!
//! subject to `decode(archive9) == enwik9` byte-for-byte and every Hutter
//! resource, portability, publication and self-containment constraint. A
//! mechanism is admitted only when its *complete* marginal cost `ΔS(M) < 0`.
//!
//! ## Two planes
//!
//! * The **research/compiler plane** may be arbitrarily large and slow: it may
//!   use threads, SIMD, GPUs, huge RAM and expensive search to *discover* the
//!   final representation.
//! * The **submission plane** is sacred: deterministic, self-contained, compact,
//!   CPU-only, resource-bounded, exact, reproducible and byte-accounted. It
//!   carries only profitable truth.
//!
//! This crate hosts both. Modules that exist only for discovery say so.

#![forbid(unsafe_code)]

pub mod archive;
pub mod context;
pub mod corpus;
pub mod entropy;
pub mod evidence;
#[cfg(feature = "grammar")]
pub mod grammar;
pub mod ir;
pub mod memory;
pub mod mixer;
// Phase 8: the learned residual corrector (integer inference; `include_bytes!`
// model weights). Behind a feature so a rejected corrector costs no bytes.
#[cfg(feature = "learned")]
pub mod learned;
pub mod score;
// Phase 14: the causal structural-state interface (SignalBus). Every field is a
// pure function of bytes already coded, so it is legal on the decoded path.
//
// **Gated to the research plane for now**: nothing in the accepted pipeline reads
// it yet, so compiling it into the scored stub would be a claim about the binary
// that only the symbol table could confirm. When a consumer that pays for itself
// reads it, it moves into `accepted` and its marginal cost is measured like any
// other mechanism.
#[cfg(any(feature = "procedural", feature = "opportunity"))]
pub mod signal;
// Phase 14: the bounded procedural DSL — programs, exact serialization,
// target-directed search and typed residuals. Research-plane until a
// constructed representation beats the accepted one on the authority corpus.
#[cfg(feature = "procedural")]
pub mod procedural;
// Phase 14.9: the boundary experiment — is any real Wikipedia class cheaper as a
// shared program plus state plus residual than as the accepted representation?
// Research-plane, and it answers the question with a measurement and its
// falsifying control rather than with an argument.
#[cfg(feature = "procedural")]
pub mod procedure;
// Phase 14: where the codelength actually lives. Research-plane (its
// accumulator is `f64`), and it asserts that every coded byte is attributed.
#[cfg(feature = "opportunity")]
pub mod opportunity;
// Research-plane progress reporting for long coding passes. Deliberately outside
// the `accepted` feature bundle: the scored stub carries none of it.
#[cfg(feature = "progress")]
pub mod progress;
// Phase 9: global search — the runtime hyperparameter space, the frf-fuzz
// mutation operator, the DSFB observer and the Gemel-memory query helpers. This
// is research-plane machinery: the scored path needs only
// `context::APM_RATE_SETS`, so the module is gated out of the submission stub.
#[cfg(not(feature = "submission"))]
pub mod search;
// Phase 7: the article-layout compiler (encoder-side orderings + the decoder
// page-id sort). Compiled behind a feature so the rejected orderings do not
// cost submission bytes.
#[cfg(feature = "reorder")]
pub mod reorder;
// The transform module is needed by either transform-based feature.
#[cfg(any(feature = "struct-hoist", feature = "alphabet-perm"))]
pub mod transform;

/// Crate version, surfaced in receipts.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The project's defining equation, as a doc constant so it cannot drift from
/// the code's intent.
pub const DEFINING_EQUATION: &str = "enwik9 = Materialize(deterministic_structure, reusable_knowledge, configuration_state, learned_residual_model, irreducible_innovation)";

#[cfg(test)]
mod integration {
    use crate::archive::{decode, encode};

    #[test]
    fn end_to_end_roundtrip_small() {
        let data =
            b"<page><title>Zentropy</title>\n{{cite|a=1}} [[Link]] text 123 &amp;\n</page>\n";
        let arch = encode(data);
        let back = decode(&arch).expect("decode");
        assert_eq!(back, data);
    }

    #[test]
    fn end_to_end_roundtrip_repetitive() {
        let mut data = Vec::new();
        for i in 0..500 {
            data.extend_from_slice(
                format!("line {i}: the quick brown fox jumps over the lazy dog\n").as_bytes(),
            );
        }
        let arch = encode(&data);
        let back = decode(&arch).expect("decode");
        assert_eq!(back, data);
        // Repetitive input must compress.
        assert!(
            arch.len() < data.len() / 3,
            "arch={} data={}",
            arch.len(),
            data.len()
        );
    }

    #[test]
    fn end_to_end_roundtrip_binary() {
        let data: Vec<u8> = (0..=255u8).cycle().take(20_000).collect();
        let arch = encode(&data);
        let back = decode(&arch).expect("decode");
        assert_eq!(back, data);
    }
}
