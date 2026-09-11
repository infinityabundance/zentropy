//! Phase 3: structural hoisting.
//!
//! `X = Reconstruct(...) ⊕ R`. The cheapest mechanism is one that does not
//! predict a value at all: it regenerates it from state already represented
//! (§19). This transform hoists frequent *structural* strings out of the
//! modelled stream into single-byte codes.
//!
//! Design constraints, in priority order:
//!
//! 1. **Exactness.** The transform is a bijection on byte strings with a
//!    universal escape, so `decode(encode(x)) == x` for arbitrary input,
//!    including malformed Wikipedia markup and binary noise.
//! 2. **Zero metadata.** The dictionary lives in the decoder's `.rodata`, so its
//!    cost is binary bytes (small, fixed), not per-archive metadata.
//! 3. **Honesty.** The transform is *not* assumed to help. Its complete
//!    `ΔS = Δarchive + Δbinary` is measured; it is adopted only if negative.
//!
//! Byte codes 0x01..=0x1F address 31 dictionary entries. 0x00 is the escape
//! marker: a literal reserved byte is written as `0x00, byte`. Any literal
//! byvalue <= 0x1F or == 0x7F is escaped. All other bytes are copied.

/// Dictionary of hoisted structural strings, longest-match-first ordering is
/// resolved at match time rather than by list order.
pub const DICT: [&[u8]; 31] = [
    b"<page>",
    b"</page>",
    b"<title>",
    b"</title>",
    b"<text",
    b"</text>",
    b"<revision>",
    b"</revision>",
    b"<id>",
    b"</id>",
    b"<timestamp>",
    b"</timestamp>",
    b"<contributor>",
    b"</contributor>",
    b"<username>",
    b"</username>",
    b"<comment>",
    b"</comment>",
    b"<minor />",
    b"&amp;",
    b"&quot;",
    b"&lt;",
    b"&gt;",
    b"&#",
    b"{{",
    b"}}",
    b"[[",
    b"]]",
    b"http://",
    b"https://",
    b"</mediawiki>",
];

/// Escape marker.
const ESC: u8 = 0x00;
/// Highest code used for a dictionary entry (`DICT.len()`).
const MAX_CODE: u8 = DICT.len() as u8;

#[inline]
fn is_reserved(b: u8) -> bool {
    b <= MAX_CODE || b == 0x7f
}

/// Hoist structural strings. Output is a valid byte stream over the same
/// alphabet; `decode` inverts it exactly.
pub fn encode(input: &[u8]) -> Vec<u8> {
    // Build an index by first byte so matching is cheap.
    let mut out = Vec::with_capacity(input.len());
    let n = input.len();
    let mut i = 0usize;
    while i < n {
        let b = input[i];
        // Try the longest dictionary match at this position.
        let mut best: Option<(usize, u8)> = None; // (len, code)
        for (idx, entry) in DICT.iter().enumerate() {
            let l = entry.len();
            if l <= n - i && &input[i..i + l] == *entry {
                let code = (idx + 1) as u8;
                match best {
                    Some((bl, _)) if bl >= l => {}
                    _ => best = Some((l, code)),
                }
            }
        }
        if let Some((l, code)) = best {
            out.push(code);
            i += l;
        } else if is_reserved(b) {
            out.push(ESC);
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`encode`].
pub fn decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 2);
    let mut i = 0usize;
    while i < input.len() {
        let b = input[i];
        if b == ESC {
            // A literal reserved byte follows.
            if i + 1 < input.len() {
                out.push(input[i + 1]);
                i += 2;
            } else {
                // Malformed tail: emit the escape verbatim. The encoder never
                // produces this, but decode must stay total.
                out.push(ESC);
                i += 1;
            }
        } else if b >= 1 && b <= MAX_CODE {
            out.extend_from_slice(DICT[(b - 1) as usize]);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// NOTE: there is deliberately no `binary_cost_estimate()` here.
///
/// The executable cost of this mechanism is established by building an
/// otherwise-identical submission binary with and without the `struct-hoist`
/// feature and charging the measured delta (see `tools/measure_binary_cost.sh`).
/// An estimate is never permitted to decide adoption.

/// A2 — frequency-ordered 256-symbol permutation.
///
/// Maps each byte value to a code such that more frequent symbols receive
/// smaller codes. For a bitwise predictor this changes *which binary
/// distinctions are asked first*, which is not invariant the way it is for an
/// ideal byte coder. Ties break by symbol value for determinism.
pub fn frequency_perm(input: &[u8]) -> [u8; 256] {
    let mut counts = [0u64; 256];
    for &b in input {
        counts[b as usize] += 1;
    }
    let mut order: Vec<u8> = (0..=255u8).collect();
    order.sort_by(|&a, &b| counts[b as usize].cmp(&counts[a as usize]).then(a.cmp(&b)));
    let mut perm = [0u8; 256];
    for (code, &sym) in order.iter().enumerate() {
        perm[sym as usize] = code as u8;
    }
    perm
}

/// A2 negative control: a deterministic pseudo-random permutation.
pub fn random_perm(seed: u64) -> [u8; 256] {
    let mut p: [u8; 256] = core::array::from_fn(|i| i as u8);
    let mut s = seed | 1;
    for i in (1..256).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let j = (s as usize) % (i + 1);
        p.swap(i, j);
    }
    p
}

/// Apply a symbol→code permutation.
pub fn apply_perm(input: &[u8], perm: &[u8; 256]) -> Vec<u8> {
    input.iter().map(|&b| perm[b as usize]).collect()
}

/// Invert a symbol→code permutation into a code→symbol table.
pub fn invert_perm(perm: &[u8; 256]) -> [u8; 256] {
    let mut inv = [0u8; 256];
    for (sym, &code) in perm.iter().enumerate() {
        inv[code as usize] = sym as u8;
    }
    inv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let enc = encode(data);
        let dec = decode(&enc);
        assert_eq!(dec, data, "transform roundtrip failed");
    }

    #[test]
    fn roundtrip_markup() {
        roundtrip(b"<page><title>Foo</title><id>42</id></page>\n<page></page>");
    }

    #[test]
    fn roundtrip_reserved_bytes() {
        let data: Vec<u8> = (0..=255u8).collect();
        roundtrip(&data);
    }

    #[test]
    fn roundtrip_truncated_markup() {
        for s in [
            &b"<page"[..],
            &b"</page"[..],
            &b"<title>unterminated"[..],
            &b"&amp"[..],
            &b"{{"[..],
            &b"\x00\x01\x1f\x7f"[..],
        ] {
            roundtrip(s);
        }
    }

    #[test]
    fn shrinks_markup_heavy_text() {
        let data = b"<page><title>X</title><id>1</id></page>\n".repeat(100);
        assert!(encode(&data).len() < data.len() / 2);
        roundtrip(&data);
    }

    #[test]
    fn perm_roundtrips_and_ranks() {
        let data = b"aaaaabbbbccd";
        let perm = frequency_perm(data);
        // 'a' is most frequent, so it must receive the smallest code.
        assert_eq!(perm[b'a' as usize], 0);
        let enc = apply_perm(data, &perm);
        let inv = invert_perm(&perm);
        let dec = apply_perm(&enc, &inv);
        assert_eq!(dec, data);
    }

    #[test]
    fn perm_is_a_bijection_for_all_bytes() {
        for perm in [frequency_perm(b"hello world"), random_perm(42)] {
            let mut seen = [false; 256];
            for &c in &perm {
                assert!(!seen[c as usize], "permutation is not injective");
                seen[c as usize] = true;
            }
        }
    }

    #[test]
    fn random_perm_matches_roundtrip() {
        let data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        let perm = random_perm(7);
        let enc = apply_perm(&data, &perm);
        let inv = invert_perm(&perm);
        assert_eq!(apply_perm(&enc, &inv), data);
    }
}
