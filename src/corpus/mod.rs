//! Canonical corpus acquisition, provenance and the research corpus ladder.
//!
//! Phase 0 requires that the exact `enwik9` bytes and their cryptographic digest
//! are pinned before any compression claim is made. This module owns that
//! knowledge and the ladder used to stage experiments without extrapolating
//! linearly from tiny fixtures to the full corpus (§26).

pub mod sha256;

use std::fs;
use std::io;
use std::path::Path;

pub use sha256::{hex, sha256, Sha256};

/// enwik9 is defined by the competition as the first 10^9 bytes of the English
/// Wikipedia XML dump of 2006-03-03. It is exactly one gigabyte, decimal.
pub const ENWIK9_LEN: u64 = 1_000_000_000;

/// The research corpus ladder, in ascending order of size.
///
/// A mechanism promoted on a rung is *not* claimed to win on a larger rung; the
/// ladder exists to make failures cheap and attribution meaningful. Full-corpus
/// runs happen only at milestone gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    /// Micro fixture (hand-written, committed under `fixtures/`).
    Fixture,
    /// First 10^6 bytes.
    Enwik6,
    /// First 10^7 bytes.
    Enwik7,
    /// First 10^8 bytes (the legacy Hutter Prize corpus).
    Enwik8,
    /// Representative regions of the full corpus.
    Regions,
    /// The full 10^9-byte corpus.
    Enwik9,
}

impl Rung {
    /// The number of bytes this rung consumes from the head of enwik9, if the
    /// rung is a simple prefix. `Fixture` and `Regions` return `None`.
    pub fn prefix_len(self) -> Option<u64> {
        match self {
            Rung::Enwik6 => Some(1_000_000),
            Rung::Enwik7 => Some(10_000_000),
            Rung::Enwik8 => Some(100_000_000),
            Rung::Enwik9 => Some(ENWIK9_LEN),
            Rung::Fixture | Rung::Regions => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Rung::Fixture => "fixture",
            Rung::Enwik6 => "enwik6",
            Rung::Enwik7 => "enwik7",
            Rung::Enwik8 => "enwik8",
            Rung::Regions => "regions",
            Rung::Enwik9 => "enwik9",
        }
    }
}

/// A verified corpus observation: the bytes, their length and their digest.
///
/// This is the atom of corpus provenance. Every experiment receipt binds one of
/// these so that "which bytes did we actually run on?" is never ambiguous.
#[derive(Debug, Clone)]
pub struct CorpusReceipt {
    /// Logical name, e.g. `"enwik8"` or a fixture filename.
    pub name: String,
    /// Exact byte length.
    pub len: u64,
    /// SHA-256 of the exact bytes.
    pub sha256: [u8; 32],
}

impl CorpusReceipt {
    /// Build a receipt for a byte slice.
    pub fn of(name: &str, data: &[u8]) -> Self {
        CorpusReceipt {
            name: name.to_string(),
            len: data.len() as u64,
            sha256: sha256(data),
        }
    }

    /// Hex digest, lowercase.
    pub fn hex_digest(&self) -> String {
        hex(&self.sha256)
    }

    /// One-line, deterministic rendering used in evidence records.
    pub fn render(&self) -> String {
        format!(
            "{} len={} sha256={}",
            self.name,
            self.len,
            self.hex_digest()
        )
    }
}

/// Provenance of the canonical `enwik9.zip` distribution.
///
/// The competition's canonical task is the *decompressed* 10^9-byte file. We
/// additionally pin the distribution archive so that two independent
/// acquisitions can be compared. The published SHA-256 of `enwik9.zip` is
/// recorded in `evidence/baseline/CORPUS.sha256` after the first verified
/// acquisition and is checked here when a manifest is available.
pub struct Canonical;

impl Canonical {
    /// Expected on-disk length of the decompressed corpus.
    pub fn expected_len() -> u64 {
        ENWIK9_LEN
    }

    /// Pinned SHA-256 digests of the canonical corpus files, measured during
    /// the first verified Phase-0 acquisition (2026-09-11, from
    /// `https://mattmahoney.net/dc/`). Recorded in
    /// `evidence/baseline/CORPUS.sha256`.
    ///
    /// `enwik6`/`enwik7` are defined as prefixes of `enwik8`/`enwik9` and are
    /// verified by construction (same head bytes).
    pub const KNOWN_SHA256_ENWIK8: &'static str =
        "2b49720ec4d78c3c9fabaee6e4179a5e997302b3a70029f30f2d582218c024a8";
    pub const KNOWN_SHA256_ENWIK9: &'static str =
        "159b85351e5f76e60cbe32e04c677847a9ecba3adc79addab6f4c6c7aa3744bc";

    /// Check a receipt against the pinned digest for a named canonical file.
    /// Returns `None` if the name is not a pinned canonical file.
    pub fn verify(receipt: &CorpusReceipt) -> Option<bool> {
        let expected = match receipt.name.as_str() {
            "enwik8" => Self::KNOWN_SHA256_ENWIK8,
            "enwik9" => Self::KNOWN_SHA256_ENWIK9,
            _ => return None,
        };
        Some(
            receipt.hex_digest() == expected
                && receipt.len
                    == if receipt.name == "enwik9" {
                        ENWIK9_LEN
                    } else {
                        100_000_000
                    },
        )
    }
}

/// Read a corpus file into memory, returning the bytes and a receipt.
pub fn load(path: impl AsRef<Path>) -> io::Result<(Vec<u8>, CorpusReceipt)> {
    let path = path.as_ref();
    let data = fs::read(path)?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unknown>".to_string());
    let receipt = CorpusReceipt::of(&name, &data);
    Ok((data, receipt))
}

/// Take the first `n` bytes of a buffer as a ladder rung.
pub fn prefix(data: &[u8], n: u64) -> &[u8] {
    let n = n.min(data.len() as u64) as usize;
    &data[..n]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_is_stable() {
        let r = CorpusReceipt::of("x", b"abc");
        assert_eq!(r.len, 3);
        assert_eq!(
            r.hex_digest(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(r.render().contains("len=3"));
    }

    #[test]
    fn prefix_clamps() {
        let d = b"hello";
        assert_eq!(prefix(d, 3), b"hel");
        assert_eq!(prefix(d, 99), b"hello");
    }

    #[test]
    fn canonical_lengths() {
        assert_eq!(Rung::Enwik6.prefix_len(), Some(1_000_000));
        assert_eq!(Rung::Enwik7.prefix_len(), Some(10_000_000));
        assert_eq!(Rung::Enwik8.prefix_len(), Some(100_000_000));
        assert_eq!(Rung::Enwik9.prefix_len(), Some(ENWIK9_LEN));
    }
}
