//! The archive container: the scored second half of `S`.
//!
//! Format (little-endian unless stated):
//!
//! ```text
//! offset  size  field
//! 0       4     magic = b"ZNT0"
//! 4       1     method (0 = raw bytes through the context-mixing floor)
//! 5       8     original_len (u64)
//! 13      ...   payload
//! ```
//!
//! The model configuration is a deterministic function of `original_len`, so it
//! is not stored; this is correct only while that function is stable, which is
//! why it lives in one place ([`crate::context::ModelConfig::for_size`]).
//!
//! The decoder bounds expansion by the declared `original_len` and rejects
//! absurd lengths before allocating (§42), so a corrupt archive cannot cause an
//! uncontrolled allocation.

use crate::context::{Cm, ModelConfig};
use crate::entropy::{RangeDecoder, RangeEncoder};

/// Container magic.
pub const MAGIC: &[u8; 4] = b"ZNT0";
/// Fixed header length.
pub const HEADER_LEN: usize = 4 + 1 + 8;

/// Coding method selector. New methods are added as phases land; each must be
/// independently exact. The variants differ only in the expert set, so they are
/// the ablation surface for the classical floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Method {
    /// Raw bytes through the full context-mixing floor (orders + word/bigram + match).
    RawCm = 0,
    /// Same floor without the word and word-bigram experts (ablation).
    RawCmNoWord = 1,
}

impl Method {
    fn config(self, n: usize) -> ModelConfig {
        match self {
            Method::RawCm => ModelConfig::for_size(n as u64),
            Method::RawCmNoWord => {
                // Keep every byte-order expert; drop the word and bigram experts
                // (the last two specs).
                let full = ModelConfig::for_size(n as u64);
                let keep: Vec<usize> = (0..full.specs.len().saturating_sub(2)).collect();
                full.ablated(&keep)
            }
        }
    }
}

/// Upper bound on a declared output length, to bound allocations on corrupt
/// input. The canonical corpus is 10^9 bytes; 2^31 gives headroom for dev
/// slices without permitting an unbounded expansion.
pub const MAX_OUTPUT: u64 = 2_000_000_000;

/// Marker written by the SFX packager, immediately followed by an 8-byte
/// little-endian archive length and then the archive bytes. The marker is
/// improbable in XML text, and the packager/reader share it so the format
/// cannot drift.
pub const SFX_MARKER: &[u8; 15] = b"\0ZNTROPY-SFX\0\0\0";

/// Append an archive to an executable image, producing a self-extracting file.
pub fn append_sfx(image: &mut Vec<u8>, archive: &[u8]) {
    image.extend_from_slice(SFX_MARKER);
    image.extend_from_slice(&(archive.len() as u64).to_le_bytes());
    image.extend_from_slice(archive);
}

/// Recover the appended archive from a self-extracting image. The **last**
/// marker wins, so a byte sequence resembling the marker inside the image
/// cannot shadow the real payload.
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

/// Compress `input` into an archive payload.
pub fn encode(input: &[u8]) -> Vec<u8> {
    encode_with(input, Method::RawCm)
}

/// Compress with an explicit method (used by ablation runs).
pub fn encode_with(input: &[u8], method: Method) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + input.len() / 2);
    out.extend_from_slice(MAGIC);
    out.push(method as u8);
    out.extend_from_slice(&(input.len() as u64).to_le_bytes());

    let cfg = method.config(input.len());
    let mut cm = Cm::new(&cfg, input.len());
    let mut enc = RangeEncoder::with_capacity(input.len() / 2 + 64);

    for &byte in input {
        let mut mask = 0x80u32;
        while mask != 0 {
            // MSB first. Compare against the mask rather than shifting it by a
            // constant, which would collapse every bit after the first.
            let bit = if (byte as u32) & mask != 0 { 1 } else { 0 };
            let p = cm.predict();
            enc.encode(bit, p);
            cm.update(bit);
            mask >>= 1;
        }
    }
    let payload = enc.finish();
    out.extend_from_slice(&payload);
    out
}

/// Decode an archive payload produced by [`encode`].
///
/// Returns `None` for a malformed container (bad magic, unknown method, or an
/// implausible length). A valid container always yields exactly
/// `original_len` bytes; the caller verifies equality against the expected
/// digest in the exactness court.
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
        _ => return None,
    };
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&archive[5..13]);
    let n = u64::from_le_bytes(len_bytes);
    if n > MAX_OUTPUT {
        return None;
    }
    let n = n as usize;
    let payload = &archive[HEADER_LEN..];

    let cfg = method.config(n);
    let mut cm = Cm::new(&cfg, n);
    let mut dec = RangeDecoder::new(payload);
    let mut out = Vec::with_capacity(n);

    for _ in 0..n {
        let mut byte = 0u32;
        for _ in 0..8 {
            let p = cm.predict();
            let bit = dec.decode(p);
            cm.update(bit);
            byte = (byte << 1) | bit;
        }
        out.push(byte as u8);
    }
    Some(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_well_formed() {
        let arch = encode(b"hello");
        assert_eq!(&arch[0..4], MAGIC);
        assert_eq!(arch[4], Method::RawCm as u8);
        assert_eq!(u64::from_le_bytes(arch[5..13].try_into().unwrap()), 5);
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(decode(b"").is_none());
        assert!(decode(b"XXXX\x00\x00\x00\x00\x00\x00\x00\x00\x00").is_none());
        // Implausible length is rejected before allocation.
        let mut a = Vec::new();
        a.extend_from_slice(MAGIC);
        a.push(0);
        a.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode(&a).is_none());
    }

    #[test]
    fn roundtrip_all_byte_values() {
        let data: Vec<u8> = (0..=255u8).collect();
        let arch = encode(&data);
        assert_eq!(decode(&arch).unwrap(), data);
    }

    #[test]
    fn ablation_method_is_exact() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(50);
        for m in [Method::RawCm, Method::RawCmNoWord] {
            let arch = encode_with(&data, m);
            assert_eq!(decode(&arch).unwrap(), data, "method {m:?}");
        }
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
        img.extend_from_slice(SFX_MARKER); // a decoy inside the image
        let arch = b"real".to_vec();
        append_sfx(&mut img, &arch);
        assert_eq!(extract_sfx(&img).unwrap(), &arch[..]);
    }
}
