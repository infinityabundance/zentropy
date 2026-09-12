//! The archive container: the scored second half of `S`.
//!
//! Format (little-endian unless stated):
//!
//! ```text
//! offset  size  field
//! 0       4     magic = b"ZNT0"
//! 4       1     method
//! 5       1     tune (optimizer/learning-rate variant; A20)
//! 6       8     coded_len (u64)  -- bytes the context model codes
//! 14      256   symbol permutation (only for methods that permute the alphabet)
//! ...           payload
//! ```
//!
//! The model configuration is a deterministic function of `(method, coded_len)`,
//! so it is not stored; this is correct only while that function is stable,
//! which is why it lives in one place ([`Method::config`]).
//!
//! Optimization Phase A adds mechanisms as *methods* so each is individually
//! ablatable and so the interaction matrix (A28) can be built by adding
//! variants. Every method must reconstruct exactly; the exactness court checks
//! the whole cross-product.

use crate::context::{Cm, InfoMode, ModelConfig};
use crate::entropy::{RangeDecoder, RangeEncoder};

/// Container magic.
pub const MAGIC: &[u8; 4] = b"ZNT0";
/// Fixed header length (excluding any permutation table).
pub const HEADER_LEN: usize = 4 + 1 + 1 + 8;
/// Length of a stored symbol permutation.
pub const PERM_LEN: usize = 256;

/// Upper bound on a declared output length, to bound allocations on corrupt
/// input. The canonical corpus is 10^9 bytes; 2^31 gives headroom for dev
/// slices without permitting an unbounded expansion.
pub const MAX_OUTPUT: u64 = 2_000_000_000;

/// Which refinement a permutation belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermKind {
    None,
    /// A2: frequency-ordered assignment.
    Freq,
    /// A2 negative control: deterministic pseudo-random assignment.
    Random,
}

/// Coding method. New mechanisms are added as variants so each is ablatable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Method {
    /// Full floor, no transforms (word + match experts only).
    RawCm = 0,
    /// Floor without the word and word-bigram experts (ablation).
    RawCmNoWord = 1,
    /// Phase 3: structural hoisting + full floor.
    StructHoist = 2,
    /// A2: structural hoisting + frequency-ordered alphabet permutation.
    AlphabetPerm = 3,
    /// A2 negative control: random alphabet permutation.
    AlphabetRandom = 4,
    /// A3: structural hoisting + information inheritance.
    InfoInherit = 5,
    /// A3 negative control: inheritance from an unrelated expert.
    InfoUnrelated = 6,
    /// A2 + A3 combined.
    AlphabetPermInfo = 7,
    /// A17: structural hoisting + previous-line/column expert.
    Column = 8,
    /// A17 negative control: wrong-column expert.
    ColumnShuffled = 9,
    /// A17 null control: no previous-line byte (left + column only).
    ColumnNoLine = 10,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::RawCm => "rawcm",
            Method::RawCmNoWord => "rawcm-noword",
            Method::StructHoist => "struct-hoist",
            Method::AlphabetPerm => "alphabet-perm",
            Method::AlphabetRandom => "alphabet-random",
            Method::InfoInherit => "info-inherit",
            Method::InfoUnrelated => "info-unrelated",
            Method::AlphabetPermInfo => "alphabet-perm+info-inherit",
            Method::Column => "column",
            Method::ColumnShuffled => "column-shuffled",
            Method::ColumnNoLine => "column-noline",
        }
    }

    pub fn from_name(s: &str) -> Option<Method> {
        Some(match s {
            "rawcm" => Method::RawCm,
            "rawcm-noword" => Method::RawCmNoWord,
            "struct-hoist" => Method::StructHoist,
            "alphabet-perm" => Method::AlphabetPerm,
            "alphabet-random" => Method::AlphabetRandom,
            "info-inherit" => Method::InfoInherit,
            "info-unrelated" => Method::InfoUnrelated,
            "alphabet-perm+info-inherit" => Method::AlphabetPermInfo,
            "column" => Method::Column,
            "column-shuffled" => Method::ColumnShuffled,
            "column-noline" => Method::ColumnNoLine,
            _ => return None,
        })
    }

    /// All methods, for exhaustive exactness testing.
    pub const ALL: [Method; 11] = [
        Method::RawCm,
        Method::RawCmNoWord,
        Method::StructHoist,
        Method::AlphabetPerm,
        Method::AlphabetRandom,
        Method::InfoInherit,
        Method::InfoUnrelated,
        Method::AlphabetPermInfo,
        Method::Column,
        Method::ColumnShuffled,
        Method::ColumnNoLine,
    ];

    /// Whether this method runs the structural-hoisting transform.
    fn hoists(self) -> bool {
        let wants = matches!(
            self,
            Method::StructHoist
                | Method::AlphabetPerm
                | Method::AlphabetRandom
                | Method::InfoInherit
                | Method::InfoUnrelated
                | Method::AlphabetPermInfo
                | Method::Column
                | Method::ColumnShuffled
                | Method::ColumnNoLine
        );
        cfg!(feature = "struct-hoist") && wants
    }

    /// Which alphabet permutation this method applies, if any.
    fn perm(self) -> PermKind {
        if !cfg!(feature = "alphabet-perm") {
            return PermKind::None;
        }
        match self {
            Method::AlphabetPerm | Method::AlphabetPermInfo => PermKind::Freq,
            Method::AlphabetRandom => PermKind::Random,
            _ => PermKind::None,
        }
    }

    /// A3 information-inheritance mode.
    fn info(self) -> InfoMode {
        if !cfg!(feature = "info-inherit") {
            return InfoMode::None;
        }
        match self {
            Method::InfoInherit | Method::AlphabetPermInfo => InfoMode::Inherit,
            Method::InfoUnrelated => InfoMode::Unrelated,
            _ => InfoMode::None,
        }
    }

    fn config(self, n: usize) -> ModelConfig {
        let base = ModelConfig::for_size(n as u64);
        let base = match self {
            Method::RawCmNoWord => {
                let keep: Vec<usize> = (0..base.specs.len().saturating_sub(2)).collect();
                base.ablated(&keep)
            }
            _ => base,
        };
        let base = match self {
            Method::Column => base.with_column(false),
            Method::ColumnShuffled => base.with_column(true),
            Method::ColumnNoLine => base.with_column_kind(crate::context::CtxKind::ColumnNoLine),
            _ => base,
        };
        base.with_info(self.info())
    }
}

// --- transform plumbing, feature-gated so binary cost is measurable ---------

#[cfg(feature = "struct-hoist")]
fn maybe_hoist(method: Method, input: &[u8]) -> Vec<u8> {
    if method.hoists() {
        crate::transform::encode(input)
    } else {
        input.to_vec()
    }
}

#[cfg(not(feature = "struct-hoist"))]
fn maybe_hoist(_method: Method, input: &[u8]) -> Vec<u8> {
    input.to_vec()
}

#[cfg(feature = "struct-hoist")]
fn maybe_unhoist(method: Method, data: Vec<u8>) -> Vec<u8> {
    if method.hoists() {
        crate::transform::decode(&data)
    } else {
        data
    }
}

#[cfg(not(feature = "struct-hoist"))]
fn maybe_unhoist(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

#[cfg(feature = "alphabet-perm")]
fn maybe_perm(method: Method, data: Vec<u8>) -> (Vec<u8>, Option<[u8; PERM_LEN]>) {
    match method.perm() {
        PermKind::None => (data, None),
        PermKind::Freq => {
            let p = crate::transform::frequency_perm(&data);
            let e = crate::transform::apply_perm(&data, &p);
            (e, Some(p))
        }
        PermKind::Random => {
            let p = crate::transform::random_perm(0x5EED_5EED);
            let e = crate::transform::apply_perm(&data, &p);
            (e, Some(p))
        }
    }
}

#[cfg(not(feature = "alphabet-perm"))]
fn maybe_perm(_method: Method, data: Vec<u8>) -> (Vec<u8>, Option<[u8; PERM_LEN]>) {
    (data, None)
}

#[cfg(feature = "alphabet-perm")]
fn maybe_unperm(data: Vec<u8>, perm: Option<&[u8; PERM_LEN]>) -> Vec<u8> {
    match perm {
        Some(p) => {
            let inv = crate::transform::invert_perm(p);
            crate::transform::apply_perm(&data, &inv)
        }
        None => data,
    }
}

#[cfg(not(feature = "alphabet-perm"))]
fn maybe_unperm(data: Vec<u8>, _perm: Option<&[u8; PERM_LEN]>) -> Vec<u8> {
    data
}

// --- codec ------------------------------------------------------------------

/// The method and tune of the currently accepted candidate. `encode` uses these
/// so the production path (bench, compress, SFX) always reflects the best
/// measured configuration. Optimization-A experiments override both explicitly.
pub const ACCEPTED_METHOD: Method = Method::Column;
/// A20: mixer learning rate 24 was adopted on enwik7 screening and confirmed on
/// enwik8 (−76,483 B at zero executable cost).
pub const ACCEPTED_TUNE: u8 = 7;

/// Compress `input` into an archive payload using the accepted configuration.
pub fn encode(input: &[u8]) -> Vec<u8> {
    encode_tuned(input, ACCEPTED_METHOD, ACCEPTED_TUNE)
}

/// Compress with an explicit method (used by ablation and Phase-A runs).
pub fn encode_with(input: &[u8], method: Method) -> Vec<u8> {
    encode_tuned(input, method, 0)
}

/// Compress with an explicit method and optimizer tuning variant (A20).
/// `tune` selects a learning-rate/update variant at *runtime*, so it costs no
/// additional executable bytes; the value is stored in the header and the
/// decoder reconstructs the identical model.
pub fn encode_tuned(input: &[u8], method: Method, tune: u8) -> Vec<u8> {
    let hoisted = maybe_hoist(method, input);
    let (data, perm) = maybe_perm(method, hoisted);
    let n = data.len();

    let mut out = Vec::with_capacity(HEADER_LEN + PERM_LEN + n / 2);
    out.extend_from_slice(MAGIC);
    out.push(method as u8);
    out.push(tune);
    out.extend_from_slice(&(n as u64).to_le_bytes());
    if let Some(p) = &perm {
        out.extend_from_slice(p);
    }

    let cfg = method.config(n).with_tune(tune);
    let mut cm = Cm::new(&cfg, n);
    let mut enc = RangeEncoder::with_capacity(n / 2 + 64);

    for &byte in data.iter() {
        let mut mask = 0x80u32;
        while mask != 0 {
            let bit = if (byte as u32) & mask != 0 { 1 } else { 0 };
            let p = cm.predict();
            enc.encode(bit, p);
            cm.update(bit);
            mask >>= 1;
        }
    }
    out.extend_from_slice(&enc.finish());
    out
}

/// Decode an archive payload produced by [`encode_with`].
pub fn decode(archive: &[u8]) -> Option<Vec<u8>> {
    if archive.len() < HEADER_LEN {
        return None;
    }
    if &archive[0..4] != MAGIC {
        return None;
    }
    let method = match archive[4] {
        0 => Method::RawCm,
        1 => Method::RawCmNoWord,
        2 => Method::StructHoist,
        3 => Method::AlphabetPerm,
        4 => Method::AlphabetRandom,
        5 => Method::InfoInherit,
        6 => Method::InfoUnrelated,
        7 => Method::AlphabetPermInfo,
        8 => Method::Column,
        9 => Method::ColumnShuffled,
        10 => Method::ColumnNoLine,
        _ => return None,
    };
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&archive[6..14]);
    let n = u64::from_le_bytes(len_bytes);
    if n > MAX_OUTPUT {
        return None;
    }
    let n = n as usize;
    let tune = archive[5];

    let mut off = HEADER_LEN;
    let perm = if method.perm() != PermKind::None {
        if archive.len() < off + PERM_LEN {
            return None;
        }
        let mut p = [0u8; PERM_LEN];
        p.copy_from_slice(&archive[off..off + PERM_LEN]);
        off += PERM_LEN;
        Some(p)
    } else {
        None
    };
    let payload = &archive[off..];

    let cfg = method.config(n).with_tune(tune);
    let mut cm = Cm::new(&cfg, n);
    let mut dec = RangeDecoder::new(payload);
    let mut decoded = Vec::with_capacity(n);
    for _ in 0..n {
        let mut byte = 0u32;
        for _ in 0..8 {
            let p = cm.predict();
            let bit = dec.decode(p);
            cm.update(bit);
            byte = (byte << 1) | bit;
        }
        decoded.push(byte as u8);
    }

    let unpermuted = maybe_unperm(decoded, perm.as_ref());
    Some(maybe_unhoist(method, unpermuted))
}

/// Peek the coded length from an archive header without decoding it. Used by
/// the memory guard to decide whether a decode can start safely.
pub fn peek_len(archive: &[u8]) -> Option<u64> {
    if archive.len() < HEADER_LEN || &archive[0..4] != MAGIC {
        return None;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&archive[6..14]);
    let n = u64::from_le_bytes(b);
    if n > MAX_OUTPUT {
        None
    } else {
        Some(n)
    }
}

/// Convenience: compress and report the score against the canonical corpus.
pub fn encode_with_report(input: &[u8]) -> (Vec<u8>, String) {
    let arch = encode(input);
    let served = crate::score::Score::new(0, arch.len() as u64);
    let report = format!(
        "input={} archive={} ratio={:.4} bits/byte(enwik9-scale)={:.4}",
        input.len(),
        arch.len(),
        input.len() as f64 / arch.len() as f64,
        served.bits_per_byte()
    );
    (arch, report)
}

/// SFX packager/reader shared format.
pub const SFX_MARKER: &[u8; 15] = b"\0ZNTROPY-SFX\0\0\0";

/// Append an archive to an executable image, producing a self-extracting file.
pub fn append_sfx(image: &mut Vec<u8>, archive: &[u8]) {
    image.extend_from_slice(SFX_MARKER);
    image.extend_from_slice(&(archive.len() as u64).to_le_bytes());
    image.extend_from_slice(archive);
}

/// Recover the appended archive. The last marker wins.
pub fn extract_sfx(image: &[u8]) -> Option<&[u8]> {
    let m = image.len().checked_sub(SFX_MARKER.len())?;
    let mut i = m;
    loop {
        if &image[i..i + SFX_MARKER.len()] == SFX_MARKER {
            let len_off = i + SFX_MARKER.len();
            if len_off + 8 > image.len() {
                return None;
            }
            let mut lb = [0u8; 8];
            lb.copy_from_slice(&image[len_off..len_off + 8]);
            let len = u64::from_le_bytes(lb) as usize;
            let start = len_off + 8;
            if start + len > image.len() {
                return None;
            }
            return Some(&image[start..start + len]);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_well_formed() {
        let arch = encode(b"hello");
        assert_eq!(&arch[0..4], MAGIC);
        // `encode` uses the accepted method/tune.
        assert_eq!(arch[4], ACCEPTED_METHOD as u8);
        assert_eq!(arch[5], ACCEPTED_TUNE);
        assert_eq!(u64::from_le_bytes(arch[6..14].try_into().unwrap()), 5);
    }

    #[test]
    fn tuning_variants_roundtrip() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(200);
        for tune in 0..8u8 {
            let arch = encode_tuned(&data, Method::StructHoist, tune);
            assert_eq!(decode(&arch).unwrap(), data, "tune={tune}");
        }
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(decode(b"").is_none());
        assert!(decode(b"XXXX\x00\x00\x00\x00\x00\x00\x00\x00\x00").is_none());
        let mut a = Vec::new();
        a.extend_from_slice(MAGIC);
        a.push(0);
        a.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode(&a).is_none());
    }

    #[test]
    fn every_method_roundtrips_exactly() {
        let data = b"<page><title>Zentropy</title> the quick brown fox 123 &amp;\n"
            .repeat(60)
            .to_vec();
        for m in Method::ALL {
            let arch = encode_with(&data, m);
            assert_eq!(decode(&arch).unwrap(), data, "method {}", m.name());
        }
    }

    #[test]
    fn every_method_roundtrips_binary() {
        let data: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
        for m in Method::ALL {
            let arch = encode_with(&data, m);
            assert_eq!(decode(&arch).unwrap(), data, "method {}", m.name());
        }
    }

    #[test]
    fn struct_hoist_shrinks_markup() {
        let data = b"<page><title>X</title><id>1</id></page>\n".repeat(100);
        let raw = encode_with(&data, Method::RawCm);
        let hoisted = encode_with(&data, Method::StructHoist);
        assert!(hoisted.len() < raw.len());
    }

    #[test]
    fn sfx_append_and_extract() {
        let mut img = b"ELF fake binary".to_vec();
        let arch = b"pretend archive payload".to_vec();
        append_sfx(&mut img, &arch);
        assert_eq!(extract_sfx(&img).unwrap(), &arch[..]);
        assert!(extract_sfx(b"no payload here").is_none());
    }

    #[test]
    fn sfx_last_marker_wins() {
        let mut img = b"x".to_vec();
        img.extend_from_slice(SFX_MARKER);
        let arch = b"real".to_vec();
        append_sfx(&mut img, &arch);
        assert_eq!(extract_sfx(&img).unwrap(), &arch[..]);
    }
}
