//! Phase 5 — procedural grammar + rank state.
//!
//! A corpus-derived grammar is an *explanation*: it removes repeated substrings
//! from the modelled stream and replaces them with rule references. The
//! constitution is explicit that this is not free:
//!
//! * the **grammar skeleton** is itself data and is written into the same
//!   modelled stream (so the CM entropy-codes it and its cost is charged);
//! * rule references are **rank-coded** when that is profitable (law 6);
//! * the body the CM sees is the **residual** after the grammar.
//!
//! Everything here is an exact bijection with a literal path, so
//! `decode(encode(x)) == x` for arbitrary input including malformed and binary.
//!
//! Symbol space: `0..=255` are literal bytes, `256 + k` is nonterminal `k`.
//!
//! Stream format:
//!
//! ```text
//! varint rule_count
//! rule_count * ( varint a , varint b )
//! u8     mtf_flag        (0 = symbols verbatim, 1 = move-to-front ranks)
//! varint body_len
//! body_len * varint symbol_or_rank
//! ```

use std::collections::HashMap;

#[inline]
fn put_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

#[inline]
fn get_varint(data: &[u8], i: &mut usize) -> u32 {
    let mut v = 0u32;
    let mut shift = 0u32;
    while *i < data.len() {
        let b = data[*i];
        *i += 1;
        v |= ((b & 0x7f) as u32) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 28 {
            break;
        }
    }
    v
}

/// Induce a byte-pair grammar. Returns the productions (in creation order) and
/// the residual body. Each round replaces the most frequent adjacent pair whose
/// count is at least `min_count`, until `max_rules` productions exist.
pub fn repair(input: &[u8], max_rules: usize, min_count: u32) -> (Vec<(u32, u32)>, Vec<u32>) {
    repair_opts(input, max_rules, min_count, false)
}

/// As [`repair`], but when `mr` is set each round substitutes the **maximal
/// repeat** containing the chosen pair (greedily extended to the right while all
/// occurrences share the same follower) rather than the pair alone — the
/// MR-RePair induction of ledger A13.
pub fn repair_opts(
    input: &[u8],
    max_rules: usize,
    min_count: u32,
    mr: bool,
) -> (Vec<(u32, u32)>, Vec<u32>) {
    let mut seq: Vec<u32> = input.iter().map(|&b| b as u32).collect();
    let mut rules: Vec<(u32, u32)> = Vec::new();
    while rules.len() < max_rules {
        let mut counts: HashMap<(u32, u32), u32> = HashMap::new();
        for w in seq.windows(2) {
            *counts.entry((w[0], w[1])).or_insert(0) += 1;
        }
        let best = counts
            .iter()
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)));
        let (&(a, b), &c) = match best {
            Some(x) if *x.1 >= min_count => x,
            _ => break,
        };
        // Optionally extend the pair to a maximal repeat on the right.
        let mut pat: Vec<u32> = vec![a, b];
        if mr {
            loop {
                let mut fol: HashMap<u32, u32> = HashMap::new();
                let l = pat.len();
                for w in seq.windows(l + 1) {
                    if w[..l] == pat[..] {
                        *fol.entry(w[l]).or_insert(0) += 1;
                    }
                }
                if fol.len() == 1 && fol.values().next() == Some(&c) {
                    pat.push(*fol.keys().next().unwrap());
                } else {
                    break;
                }
            }
        }
        // Fold the pattern into a right-leaning binary chain, or fall back to the
        // pair if the rule budget cannot hold the chain.
        let base = rules.len();
        let result_nt;
        if base + pat.len() - 1 > max_rules {
            pat.truncate(2);
        }
        if pat.len() == 2 {
            result_nt = 256 + base as u32;
            rules.push((pat[0], pat[1]));
        } else {
            for j in 1..pat.len() {
                let left = if j == 1 {
                    pat[0]
                } else {
                    256 + base as u32 + (j as u32 - 2)
                };
                rules.push((left, pat[j]));
            }
            result_nt = 256 + base as u32 + (pat.len() as u32 - 2);
        }
        let l = pat.len();
        let mut new = Vec::with_capacity(seq.len());
        let mut i = 0;
        while i < seq.len() {
            if i + l <= seq.len() && seq[i..i + l] == pat[..] {
                new.push(result_nt);
                i += l;
            } else {
                new.push(seq[i]);
                i += 1;
            }
        }
        seq = new;
    }
    (rules, seq)
}

/// Move-to-front encode a symbol sequence over the canonical alphabet `0..alphabet`.
fn mtf_encode(seq: &[u32], alphabet: usize) -> Vec<u32> {
    let mut list: Vec<u32> = (0..alphabet as u32).collect();
    let mut out = Vec::with_capacity(seq.len());
    for &s in seq {
        let pos = list.iter().position(|&x| x == s).unwrap_or(0);
        out.push(pos as u32);
        if pos > 0 {
            let v = list.remove(pos);
            list.insert(0, v);
        }
    }
    out
}

/// Inverse of [`mtf_encode`].
fn mtf_decode(seq: &[u32], alphabet: usize) -> Vec<u32> {
    let mut list: Vec<u32> = (0..alphabet as u32).collect();
    let mut out = Vec::with_capacity(seq.len());
    for &p in seq {
        let pos = (p as usize).min(list.len().saturating_sub(1));
        let v = list.remove(pos);
        out.push(v);
        list.insert(0, v);
    }
    out
}

/// Write one symbol in the verbatim format: a literal byte is itself except
/// `0xFF`, which becomes `0xFF 0x00`; a nonterminal `256+k` becomes `0xFF (k+1)`.
#[inline]
fn put_sym(out: &mut Vec<u8>, s: u32) {
    if s < 255 {
        out.push(s as u8);
    } else if s == 255 {
        out.push(0xFF);
        out.push(0x00);
    } else {
        out.push(0xFF);
        put_varint(out, s - 255);
    }
}

/// Read one symbol written by [`put_sym`]. Returns `None` on truncation.
#[inline]
fn get_sym(data: &[u8], i: &mut usize) -> Option<u32> {
    if *i >= data.len() {
        return None;
    }
    let b = data[*i];
    *i += 1;
    if b != 0xFF {
        return Some(b as u32);
    }
    let v = get_varint(data, i);
    if v == 0 {
        Some(255)
    } else {
        Some(255 + v)
    }
}

/// Encode `input` as a grammar + rank-coded residual.
pub fn encode(input: &[u8], max_rules: usize, min_count: u32, mtf: bool) -> Vec<u8> {
    encode_ex(input, max_rules, min_count, mtf, false)
}

/// As [`encode`] with an explicit induction mode (`mr` = maximal repeats).
pub fn encode_ex(input: &[u8], max_rules: usize, min_count: u32, mtf: bool, mr: bool) -> Vec<u8> {
    let (rules, body) = repair_opts(input, max_rules, min_count, mr);
    let mut out = Vec::with_capacity(input.len() / 2);
    put_varint(&mut out, rules.len() as u32);
    for &(a, b) in &rules {
        put_sym(&mut out, a);
        put_sym(&mut out, b);
    }
    out.push(u8::from(mtf));
    put_varint(&mut out, body.len() as u32);
    if mtf {
        let alphabet = 256 + rules.len();
        for r in mtf_encode(&body, alphabet) {
            put_varint(&mut out, r);
        }
    } else {
        for &s in &body {
            put_sym(&mut out, s);
        }
    }
    out
}

/// Exact inverse of [`encode`]. Total on malformed input.
pub fn decode(data: &[u8]) -> Vec<u8> {
    let mut i = 0usize;
    let rule_count = get_varint(data, &mut i) as usize;
    let mut rules: Vec<(u32, u32)> = Vec::with_capacity(rule_count);
    for _ in 0..rule_count {
        if i >= data.len() {
            break;
        }
        let a = match get_sym(data, &mut i) {
            Some(v) => v,
            None => break,
        };
        let b = match get_sym(data, &mut i) {
            Some(v) => v,
            None => break,
        };
        rules.push((a, b));
    }
    if i >= data.len() {
        return Vec::new();
    }
    let mtf = data[i] != 0;
    i += 1;
    let body_len = get_varint(data, &mut i) as usize;
    let alphabet = 256 + rules.len();
    let mut body: Vec<u32> = Vec::with_capacity(body_len);
    if mtf {
        for _ in 0..body_len {
            if i >= data.len() {
                break;
            }
            body.push(get_varint(data, &mut i));
        }
        body = mtf_decode(&body, alphabet);
    } else {
        for _ in 0..body_len {
            match get_sym(data, &mut i) {
                Some(v) => body.push(v),
                None => break,
            }
        }
    }
    // Expand nonterminals. Rule k references only symbols < 256 + k, so a simple
    // stack expansion terminates and is exact.
    let mut out = Vec::with_capacity(body_len);
    let mut stack: Vec<u32> = body.into_iter().rev().collect();
    while let Some(s) = stack.pop() {
        if s < 256 {
            out.push(s as u8);
        } else {
            let k = (s - 256) as usize;
            if k < rules.len() {
                let (a, b) = rules[k];
                stack.push(b);
                stack.push(a);
            }
        }
    }
    out
}

/// Encode with the rule table reordered by first use in the body (rank-coded rule
/// state, law 6). No extra metadata is needed: the decoder reads the rules in the
/// stored order and the body ids already refer to the new order.
pub fn encode_rrank(input: &[u8], max_rules: usize, min_count: u32) -> Vec<u8> {
    use std::collections::HashSet;
    let (rules, body) = repair(input, max_rules, min_count);
    let mut order: Vec<usize> = Vec::new();
    let mut seen: HashSet<usize> = HashSet::new();
    for &s in &body {
        if s >= 256 {
            let k = (s - 256) as usize;
            if seen.insert(k) {
                order.push(k);
            }
        }
    }
    for k in 0..rules.len() {
        if seen.insert(k) {
            order.push(k);
        }
    }
    let mut newid = vec![0u32; rules.len()];
    for (pos, &k) in order.iter().enumerate() {
        newid[k] = 256 + pos as u32;
    }
    let map = |s: u32| -> u32 {
        if s < 256 {
            s
        } else {
            newid[(s - 256) as usize]
        }
    };
    let mut out = Vec::with_capacity(input.len() / 2);
    put_varint(&mut out, order.len() as u32);
    for &k in &order {
        let (a, b) = rules[k];
        put_sym(&mut out, map(a));
        put_sym(&mut out, map(b));
    }
    out.push(0);
    put_varint(&mut out, body.len() as u32);
    for &s in &body {
        put_sym(&mut out, map(s));
    }
    out
}

// --- first-use inline productions (5.5) -------------------------------------
//
// The skeleton is emitted at each rule's first use in the body instead of in a
// header (the GLZA discipline). Byte codes: `0xFE` introduces a definition
// `0xFE varint(k) sym(a) sym(b)`; `0xFF` is the escape/nonterminal prefix:
// `0xFF 0..2` are literals `0xFD/0xFE/0xFF`, `0xFF v` with `v>=3` is
// nonterminal `256 + (v-3)`; every other byte is a literal.
#[inline]
fn put_sym_fu(out: &mut Vec<u8>, s: u32) {
    if s < 0xFD {
        out.push(s as u8);
    } else if s <= 0xFF {
        out.push(0xFF);
        out.push((s - 0xFD) as u8);
    } else {
        out.push(0xFF);
        put_varint(out, 3 + (s - 256));
    }
}

#[inline]
fn get_sym_fu(data: &[u8], i: &mut usize) -> Option<u32> {
    if *i >= data.len() {
        return None;
    }
    let b = data[*i];
    *i += 1;
    if b != 0xFF {
        return Some(b as u32);
    }
    let v = get_varint(data, i);
    Some(if v < 3 { 0xFD + v } else { 253 + v })
}

/// Emit a rule definition, recursively defining any nonterminal it references
/// first, so a decoder always sees a rule before it is used.
fn emit_rule_fu(out: &mut Vec<u8>, rules: &[(u32, u32)], defined: &mut [bool], k: usize) {
    if k >= rules.len() || defined[k] {
        return;
    }
    defined[k] = true;
    let (a, b) = rules[k];
    for s in [a, b] {
        if s >= 256 {
            emit_rule_fu(out, rules, defined, (s - 256) as usize);
        }
    }
    out.push(0xFE);
    put_varint(out, k as u32);
    put_sym_fu(out, a);
    put_sym_fu(out, b);
}

/// Encode with first-use inline productions.
pub fn encode_firstuse(input: &[u8], max_rules: usize, min_count: u32) -> Vec<u8> {
    let (rules, body) = repair(input, max_rules, min_count);
    let mut out = Vec::with_capacity(input.len() / 2);
    let mut defined = vec![false; rules.len()];
    for &s in &body {
        if s >= 256 {
            emit_rule_fu(&mut out, &rules, &mut defined, (s - 256) as usize);
        }
        put_sym_fu(&mut out, s);
    }
    out
}

/// Exact inverse of [`encode_firstuse`].
pub fn decode_firstuse(data: &[u8]) -> Vec<u8> {
    let mut i = 0usize;
    let mut rules: Vec<(u32, u32)> = Vec::new();
    let mut body: Vec<u32> = Vec::new();
    while i < data.len() {
        if data[i] == 0xFE {
            i += 1;
            let k = get_varint(data, &mut i) as usize;
            let a = match get_sym_fu(data, &mut i) {
                Some(v) => v,
                None => break,
            };
            let b = match get_sym_fu(data, &mut i) {
                Some(v) => v,
                None => break,
            };
            while rules.len() <= k {
                rules.push((0, 0));
            }
            rules[k] = (a, b);
        } else {
            match get_sym_fu(data, &mut i) {
                Some(v) => body.push(v),
                None => break,
            }
        }
    }
    let mut out = Vec::with_capacity(body.len());
    let mut stack: Vec<u32> = body.into_iter().rev().collect();
    while let Some(s) = stack.pop() {
        if s < 256 {
            out.push(s as u8);
        } else {
            let k = (s - 256) as usize;
            if k < rules.len() {
                let (a, b) = rules[k];
                stack.push(b);
                stack.push(a);
            }
        }
    }
    out
}

/// One-shot, single-pass grammar induction: select the most frequent pairs once
/// and substitute them greedily. O(n), so it scales to large corpora where the
/// iterative RePair above cannot (ledger A14's motivation). Rules reference only
/// literals, so the grammar is a single level.
pub fn repair_oneshot(
    input: &[u8],
    max_rules: usize,
    min_count: u32,
) -> (Vec<(u32, u32)>, Vec<u32>) {
    let seq: Vec<u32> = input.iter().map(|&b| b as u32).collect();
    let mut counts: HashMap<(u32, u32), u32> = HashMap::new();
    for w in seq.windows(2) {
        *counts.entry((w[0], w[1])).or_insert(0) += 1;
    }
    let mut pairs: Vec<((u32, u32), u32)> = counts
        .into_iter()
        .filter(|(_, c)| *c >= min_count)
        .collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    pairs.truncate(max_rules);
    let mut map: HashMap<(u32, u32), u32> = HashMap::new();
    let mut rules: Vec<(u32, u32)> = Vec::new();
    for (idx, (p, _)) in pairs.iter().enumerate() {
        map.insert(*p, 256 + idx as u32);
        rules.push(*p);
    }
    let mut body = Vec::with_capacity(seq.len());
    let mut i = 0;
    while i < seq.len() {
        if i + 1 < seq.len() {
            if let Some(&nt) = map.get(&(seq[i], seq[i + 1])) {
                body.push(nt);
                i += 2;
                continue;
            }
        }
        body.push(seq[i]);
        i += 1;
    }
    (rules, body)
}

/// Encode using the scalable one-shot induction.
pub fn encode_oneshot(input: &[u8], max_rules: usize, min_count: u32) -> Vec<u8> {
    let (rules, body) = repair_oneshot(input, max_rules, min_count);
    let mut out = Vec::with_capacity(input.len() / 2);
    put_varint(&mut out, rules.len() as u32);
    for &(a, b) in &rules {
        put_sym(&mut out, a);
        put_sym(&mut out, b);
    }
    out.push(0);
    put_varint(&mut out, body.len() as u32);
    for &s in &body {
        put_sym(&mut out, s);
    }
    out
}

// --- 5.8 LZ-Begin-End factorization (ledger A15) ----------------------------
//
// A factor is either a literal byte or a copy of a **contiguous sequence of
// previous factors**. The representation is at least as expressive as a grammar
// and is sometimes asymptotically smaller. This is an exact, decodable codec so
// it can be measured with the identical accounting backend rather than by an
// estimate.
//
// Stream: varint op_count, then per op a tag byte (`0` = literal + byte,
// `1` = varint start + varint count of consecutive previous factors).

/// Greedy LZBE parse. `window` bounds how many previous factors are candidates
/// and `max_run` bounds the copy length; both keep the encoder bounded.
pub fn lzbe_encode(input: &[u8], window: usize, max_run: usize) -> Vec<u8> {
    let mut factors: Vec<Vec<u8>> = Vec::new();
    let mut ops: Vec<(u8, u32, u32)> = Vec::new(); // (tag, a, b)
    let mut p = 0usize;
    while p < input.len() {
        let mut best: Option<(usize, usize, usize)> = None; // (s, m, len)
        let start = factors.len().saturating_sub(window);
        for s in start..factors.len() {
            let mut len = 0usize;
            let mut m = 0usize;
            while s + m < factors.len() && m < max_run {
                let f = &factors[s + m];
                if p + len + f.len() > input.len() {
                    break;
                }
                if &input[p + len..p + len + f.len()] == f.as_slice() {
                    len += f.len();
                    m += 1;
                } else {
                    break;
                }
            }
            if len >= 3 && best.map(|(_, _, bl)| len > bl).unwrap_or(true) {
                best = Some((s, m, len));
            }
        }
        if let Some((s, m, len)) = best {
            let mut exp = Vec::new();
            for f in &factors[s..s + m] {
                exp.extend_from_slice(f);
            }
            ops.push((1, s as u32, m as u32));
            factors.push(exp);
            p += len;
        } else {
            let b = input[p];
            ops.push((0, b as u32, 0));
            factors.push(vec![b]);
            p += 1;
        }
    }
    let mut out = Vec::new();
    put_varint(&mut out, ops.len() as u32);
    for (tag, a, b) in ops {
        out.push(tag);
        if tag == 0 {
            out.push(a as u8);
        } else {
            put_varint(&mut out, a);
            put_varint(&mut out, b);
        }
    }
    out
}

/// Exact inverse of [`lzbe_encode`].
pub fn lzbe_decode(data: &[u8]) -> Vec<u8> {
    let mut i = 0usize;
    let n = get_varint(data, &mut i) as usize;
    let mut factors: Vec<Vec<u8>> = Vec::with_capacity(n);
    for _ in 0..n {
        if i >= data.len() {
            break;
        }
        let tag = data[i];
        i += 1;
        if tag == 0 {
            if i >= data.len() {
                break;
            }
            factors.push(vec![data[i]]);
            i += 1;
        } else {
            let s = get_varint(data, &mut i) as usize;
            let m = get_varint(data, &mut i) as usize;
            if s + m > factors.len() {
                break;
            }
            let mut exp = Vec::new();
            for k in 0..m {
                exp.extend_from_slice(&factors[s + k]);
            }
            factors.push(exp);
        }
    }
    let mut out = Vec::new();
    for f in factors {
        out.extend_from_slice(&f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(data: &[u8]) {
        for (mr, mc, mtf) in [(16, 3, false), (16, 3, true), (64, 2, true)] {
            assert_eq!(
                decode(&encode(data, mr, mc, mtf)),
                data,
                "grammar roundtrip mr={mr} mc={mc} mtf={mtf}"
            );
        }
    }

    #[test]
    fn roundtrip_examples() {
        rt(b"");
        rt(b"abababab");
        rt(b"the quick brown fox jumps over the lazy dog");
        rt(b"<page><title>Zentropy</title></page>\n");
    }

    #[test]
    fn roundtrip_all_bytes_and_random() {
        rt(&(0..=255u8).collect::<Vec<u8>>());
        let mut s = 0x1234_5678_9abc_def1u64;
        for _ in 0..32 {
            let v: Vec<u8> = (0..4096)
                .map(|_| {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    (s >> 24) as u8
                })
                .collect();
            rt(&v);
        }
    }

    #[test]
    fn grammar_shrinks_repetitive_input() {
        let data = b"abcabcabcabc abcabcabcabc abcabcabcabc".repeat(10);
        let enc = encode(&data, 64, 3, false);
        assert!(enc.len() < data.len(), "no grammar gain");
        assert_eq!(decode(&enc), data);
    }

    #[test]
    fn lzbe_roundtrip() {
        for data in [
            &b""[..],
            &b"abcabcabc"[..],
            &b"the quick brown fox the quick brown fox"[..],
        ] {
            assert_eq!(lzbe_decode(&lzbe_encode(data, 256, 64)), data);
        }
        assert_eq!(
            lzbe_decode(&lzbe_encode(&(0..=255u8).collect::<Vec<u8>>(), 256, 64)),
            (0..=255u8).collect::<Vec<u8>>()
        );
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        let v: Vec<u8> = (0..8192)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 24) as u8
            })
            .collect();
        assert_eq!(lzbe_decode(&lzbe_encode(&v, 256, 64)), v);
    }
}
