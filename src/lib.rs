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
pub mod score;
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
