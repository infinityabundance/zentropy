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

/// A1.2 case-factorization markers.
pub const CASE_ESC: u8 = 0x00;
pub const CASE_TITLE: u8 = 0x01;
pub const CASE_UPPER: u8 = 0x02;
pub const CASE_MIXED: u8 = 0x03;

/// Classify an all-letters word: 0 = lower, 1 = Title, 2 = UPPER, 3 = mixed.
#[inline]
fn word_class(w: &[u8]) -> u8 {
    if w.iter().all(|b| b.is_ascii_lowercase()) {
        return 0;
    }
    if w[0].is_ascii_uppercase() && w[1..].iter().all(|b| b.is_ascii_lowercase()) {
        return 1;
    }
    if w.iter().all(|b| b.is_ascii_uppercase()) {
        return 2;
    }
    3
}

/// A1.2: separate lexical identity from orthographic case.
///
/// Lowercase words are emitted unchanged (the common case, so no expansion);
/// Title/UPPER words are emitted as a marker plus the *lowercased* word, so the
/// predictor sees one lexical form instead of many; mixed-case words are emitted
/// verbatim behind a marker (rare). Literal marker bytes are escaped.
pub fn case_encode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b.is_ascii_alphabetic() {
            let mut j = i;
            while j < input.len() && input[j].is_ascii_alphabetic() {
                j += 1;
            }
            let w = &input[i..j];
            match word_class(w) {
                0 => out.extend_from_slice(w),
                1 => {
                    out.push(CASE_TITLE);
                    for &c in w {
                        out.push(c.to_ascii_lowercase());
                    }
                }
                2 => {
                    out.push(CASE_UPPER);
                    for &c in w {
                        out.push(c.to_ascii_lowercase());
                    }
                }
                _ => {
                    out.push(CASE_MIXED);
                    out.extend_from_slice(w);
                }
            }
            i = j;
        } else if b <= CASE_MIXED {
            out.push(CASE_ESC);
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`case_encode`].
pub fn case_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == CASE_ESC {
            if i + 1 < input.len() {
                out.push(input[i + 1]);
                i += 2;
            } else {
                out.push(CASE_ESC);
                i += 1;
            }
        } else if b == CASE_TITLE {
            i += 1;
            let s = i;
            while i < input.len() && input[i].is_ascii_lowercase() {
                i += 1;
            }
            if i > s {
                out.push(input[s].to_ascii_uppercase());
                out.extend_from_slice(&input[s + 1..i]);
            }
        } else if b == CASE_UPPER {
            i += 1;
            let s = i;
            while i < input.len() && input[i].is_ascii_lowercase() {
                i += 1;
            }
            for &c in &input[s..i] {
                out.push(c.to_ascii_uppercase());
            }
        } else if b == CASE_MIXED {
            i += 1;
            let s = i;
            while i < input.len() && input[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.extend_from_slice(&input[s..i]);
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// A1.2 control: mark case but do **not** merge lexical identity (words are left
/// untouched behind a marker). Isolates the value of merging from the cost of
/// the markers themselves.
pub fn case_encode_markonly(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b.is_ascii_alphabetic() {
            let mut j = i;
            while j < input.len() && input[j].is_ascii_alphabetic() {
                j += 1;
            }
            let w = &input[i..j];
            match word_class(w) {
                0 => out.extend_from_slice(w),
                1 => {
                    out.push(CASE_TITLE);
                    out.extend_from_slice(w);
                }
                2 => {
                    out.push(CASE_UPPER);
                    out.extend_from_slice(w);
                }
                _ => {
                    out.push(CASE_MIXED);
                    out.extend_from_slice(w);
                }
            }
            i = j;
        } else if b <= CASE_MIXED {
            out.push(CASE_ESC);
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`case_encode_markonly`].
pub fn case_decode_markonly(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == CASE_ESC {
            if i + 1 < input.len() {
                out.push(input[i + 1]);
                i += 2;
            } else {
                out.push(CASE_ESC);
                i += 1;
            }
        } else if b >= CASE_TITLE && b <= CASE_MIXED {
            i += 1;
            let s = i;
            while i < input.len() && input[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.extend_from_slice(&input[s..i]);
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

// --- A1.1 / A26: dynamic frequency-ranked word vocabulary -------------------
//
// Unlike [`DICT`] (a fixed 31-entry table in `.rodata`, zero archive metadata),
// this vocabulary is derived from the input and must therefore be *stored* in
// the archive and charged. The dictionary is written as a prefix of the same
// modelled stream, so it is entropy-coded by the same predictor as the body and
// its cost is fully accounted for.
//
// Body encoding is injective:
//   * a token is `0x00 id` (id in 1..=255, two bytes),
//   * a literal `0x00` byte is escaped as `0x00 0x00`,
//   * every other byte is copied verbatim.
// `id == 0` is unused, so `0x00 0x00` is unambiguous.
pub const TOK_ESC: u8 = 0x00;
/// Largest vocabulary the one-byte id space can address (ids 1..=255).
pub const MAX_TOKENS: usize = 255;

#[inline]
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphabetic()
}

/// Phase 10: the tokenizer's word boundary rule, exposed so a measurement can
/// walk a stream with *exactly* the same notion of "word" the tokenizer uses.
/// A private duplicate would drift, and a drifted measurement is worse than none.
pub fn is_word_byte_at(b: u8) -> bool {
    is_word_byte(b)
}

/// Build the corpus-derived vocabulary.
///
/// Candidate words have length >= 3 and count >= 2, and are kept only when a
/// two-byte token recovers more than the entry's raw definition cost. Survivors
/// are ordered by descending frequency (ties by word) and truncated to the id
/// space, which makes the result deterministic regardless of hash order.
/// `reverse` flips the id assignment while keeping the same word set; it is the
/// control that isolates the value of *frequency ranking* from substitution.
pub fn build_word_vocab(input: &[u8], reverse: bool) -> Vec<Vec<u8>> {
    let counts = word_counts(input);
    let mut cand: Vec<(Vec<u8>, u64)> = counts
        .into_iter()
        .filter(|(w, c)| w.len() >= 3 && *c >= 2)
        .filter(|(w, c)| {
            let len = w.len() as i64;
            let count = *c as i64;
            // Two-byte token: save (len - 2) per occurrence, pay (len + 1) once.
            count * (len - 2) - (len + 1) > 0
        })
        .collect();
    cand.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    cand.truncate(MAX_TOKENS);
    if reverse {
        cand.reverse();
    }
    cand.into_iter().map(|(w, _)| w).collect()
}

/// Encode `input`, emitting `vocabulary || body`, with static ids.
pub fn word_token_encode(input: &[u8], reverse: bool) -> Vec<u8> {
    word_token_encode_mode(input, reverse, IdMode::Static)
}

// --- Phase 10.4 (A1.11): recency-ranked token ids ---------------------------
//
// The vocabulary header keeps the *word set* and the initial id order; the id a
// body byte carries is the token's position in a list that both sides maintain
// with the same rule. Nothing extra is stored: the list is a pure function of the
// id sequence already coded, so the transform stays invertible from the archive
// alone and costs **zero** side-stream bytes.
//
// This attacks representation *identity* rather than coverage. The accepted
// configuration already ranks ids by ascending frequency (`reverse`), and that
// choice measurably beat descending order — so the id assignment is worth
// something. Recency is the other classic candidate: recently used tokens become
// cheap. The honest risk is the opposite effect, since a rank transform destroys
// the absolute identity a context model can memorise. That is why every stream
// gets its own ablation rather than a shared verdict.

/// How a token's id is derived from its position in the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdMode {
    /// Position in the initial (frequency) order — the existing scheme.
    Static,
    /// Move-to-front: the id is the current position, and the token is promoted
    /// to the front afterwards.
    Mtf,
    /// Move-to-second: promote to position 1, leaving position 0 untouched, so a
    /// token repeated immediately keeps a stable id.
    MoveToSecond,
}

/// A permutation of the vocabulary with O(1) id lookup and O(n) promotion.
///
/// `n <= 255` always holds for the v1 vocabulary (`MAX_TOKENS` is 255 and the
/// header count is a `u8`), so ids fit in a byte by construction; the v2
/// escape-extended scheme has its own codec and is unaffected.
struct IdList {
    /// `order[position] = vocabulary index`.
    order: Vec<u8>,
    /// `pos[vocabulary index] = position`.
    pos: Vec<u8>,
}

impl IdList {
    fn new(n: usize) -> Self {
        let n = n.min(255);
        IdList {
            order: (0..n as u8).collect(),
            pos: (0..n as u8).collect(),
        }
    }

    /// The id byte for a vocabulary index (1-based; 0 is the literal-NUL escape).
    #[inline]
    fn id(&self, vi: u8) -> u8 {
        self.pos[vi as usize] + 1
    }

    /// The vocabulary index an id byte names.
    #[inline]
    fn resolve(&self, id: u8) -> u8 {
        self.order[id as usize - 1]
    }

    /// Apply the mode's promotion after a token has been used.
    #[inline]
    fn touch(&mut self, vi: u8, mode: IdMode) {
        if mode == IdMode::Static {
            return;
        }
        let p = self.pos[vi as usize] as usize;
        let target = match mode {
            IdMode::Static => p,
            IdMode::Mtf => 0,
            IdMode::MoveToSecond => 1.min(p),
        };
        if p == target {
            return;
        }
        for k in (target..p).rev() {
            let v = self.order[k];
            self.order[k + 1] = v;
            self.pos[v as usize] = (k + 1) as u8;
        }
        self.order[target] = vi;
        self.pos[vi as usize] = target as u8;
    }
}

/// Encode `input` with an explicit id mode, emitting `vocabulary || body`.
pub fn word_token_encode_mode(input: &[u8], reverse: bool, mode: IdMode) -> Vec<u8> {
    let vocab = build_word_vocab(input, reverse);
    word_token_encode_vocab(input, &vocab, mode)
}

// --- Phase 10.5 (A12.1): first-use inline definitions ---------------------
//
// The shipped tokenizer pays a **dictionary header** up front: every entry is
// defined at the very start of the stream, where the model has no context, and the
// words sit far from the text that uses them. This variant removes the header and
// defines each entry where it is first *used*.
//
// The arithmetic, before any modelling effect, per entry:
//
//   header form   1 byte (length) + word bytes   + 2 bytes per use
//   first-use      0x00 0xFF id len + word bytes + 2 bytes per later use
//
// i.e. exactly **+1 byte per entry**, less the header's count byte. For 255 entries
// that is +254 raw bytes. The only proposed benefit is locality — a definition
// spelled in its own context should be cheaper for the model than one clustered
// among 254 others at a cold start — so this is a small, honest, marginal
// experiment rather than a candidate mechanism, and it is expected to land near
// the noise floor in either direction.
//
// Ids are carried explicitly in the definition (rather than allocated in first-use
// order) so that the *id assignment* stays exactly what the shipped encoder chose.
// Experiment 10.4 measured that moving ids is catastrophic (+213,623 B at enwik7),
// so confounding this test with an id change would guarantee the wrong verdict.
// Id 255 is reserved as the definition escape, so the vocabulary is capped at 254
// entries here — one fewer than the shipped form, and stated rather than hidden.

/// Ids at or above this are unavailable to the first-use form (255 is the escape).
pub const FIRST_USE_MAX_TOKENS: usize = 254;
const FU_DEF: u8 = 0xFF;

/// Encode with no dictionary header, defining each vocabulary entry at first use.
pub fn word_token_encode_firstuse(input: &[u8], vocab: &[Vec<u8>]) -> Vec<u8> {
    use std::collections::HashMap;
    let vocab: Vec<&Vec<u8>> = vocab.iter().take(FIRST_USE_MAX_TOKENS).collect();
    let mut ids: HashMap<&[u8], u8> = HashMap::with_capacity(vocab.len());
    for (k, w) in vocab.iter().enumerate() {
        ids.insert(w.as_slice(), (k + 1) as u8);
    }
    let mut defined = vec![false; vocab.len()];
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if is_word_byte(b) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            match ids.get(&input[i..j]) {
                Some(&id) => {
                    let vi = id as usize - 1;
                    if defined[vi] {
                        out.push(TOK_ESC);
                        out.push(id);
                    } else {
                        defined[vi] = true;
                        out.push(TOK_ESC);
                        out.push(FU_DEF);
                        out.push(id);
                        out.push((j - i) as u8);
                        out.extend_from_slice(&input[i..j]);
                    }
                }
                None => out.extend_from_slice(&input[i..j]),
            }
            i = j;
        } else if b == TOK_ESC {
            out.push(TOK_ESC);
            out.push(0);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`word_token_encode_firstuse`]. Total on malformed input:
/// an undefined id is simply dropped, and a truncated definition is ignored.
pub fn word_token_decode_firstuse(data: &[u8]) -> Vec<u8> {
    let mut dict: Vec<Vec<u8>> = vec![Vec::new(); FIRST_USE_MAX_TOKENS + 1];
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let b = data[i];
        if b == TOK_ESC {
            if i + 1 >= data.len() {
                break;
            }
            let id = data[i + 1] as usize;
            if id == 0 {
                out.push(0);
                i += 2;
            } else if id == FU_DEF as usize {
                if i + 3 >= data.len() {
                    break;
                }
                let tid = data[i + 2] as usize;
                let len = data[i + 3] as usize;
                if i + 4 + len > data.len() || tid == 0 || tid > FIRST_USE_MAX_TOKENS {
                    break;
                }
                let w = data[i + 4..i + 4 + len].to_vec();
                dict[tid] = w.clone();
                out.extend_from_slice(&w);
                i += 4 + len;
            } else {
                if id <= FIRST_USE_MAX_TOKENS {
                    out.extend_from_slice(&dict[id]);
                }
                i += 2;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Encode `input` against a **given** vocabulary, with a set of words **blocked**
/// from substitution.
///
/// Phase 10 uses this to drop the *use* of a word while keeping its dictionary
/// slot, which keeps every surviving word's id unchanged. That matters: the id
/// assignment is a measured axis (A1.1), so a policy that renumbers the survivors
/// would confound "stop substituting" with "renumber", and the T1/10.4 results
/// both say the identity axis is where the damage lands.
///
/// The decoder is untouched — it expands whatever ids appear — so a blocked word
/// simply arrives as literal bytes.
pub fn word_token_encode_vocab_filtered(
    input: &[u8],
    vocab: &[Vec<u8>],
    blocked: &std::collections::HashSet<Vec<u8>>,
    mode: IdMode,
) -> Vec<u8> {
    use std::collections::HashMap;
    let mut ids: HashMap<&[u8], u8> = HashMap::with_capacity(vocab.len());
    let mut out = Vec::with_capacity(input.len());
    out.push(vocab.len() as u8);
    for (k, w) in vocab.iter().enumerate() {
        out.push(w.len() as u8);
        out.extend_from_slice(w);
        if !blocked.contains(w.as_slice()) {
            ids.insert(w.as_slice(), k as u8);
        }
    }
    let mut list = IdList::new(vocab.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if is_word_byte(b) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            match ids.get(&input[i..j]) {
                Some(&vi) => {
                    out.push(TOK_ESC);
                    out.push(list.id(vi));
                    list.touch(vi, mode);
                }
                None => out.extend_from_slice(&input[i..j]),
            }
            i = j;
        } else if b == TOK_ESC {
            out.push(TOK_ESC);
            out.push(0);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Encode `input` against a **given** vocabulary, emitting `vocabulary || body`.
///
/// Phase 10 uses this to substitute a vocabulary chosen by the model's own
/// measured costs for the one the shipped count heuristic picks. The decoder is
/// untouched: it reads the dictionary out of the stream exactly as before, which
/// is why a vocabulary change is encoder-side and cannot affect exactness.
///
/// `vocab` supplies both the word set and the initial id order, so `mode` is
/// applied on top of it.
pub fn word_token_encode_vocab(input: &[u8], vocab: &[Vec<u8>], mode: IdMode) -> Vec<u8> {
    use std::collections::HashMap;
    let mut ids: HashMap<&[u8], u8> = HashMap::with_capacity(vocab.len());
    let mut out = Vec::with_capacity(input.len());
    out.push(vocab.len() as u8);
    for (k, w) in vocab.iter().enumerate() {
        out.push(w.len() as u8);
        out.extend_from_slice(w);
        ids.insert(w.as_slice(), k as u8);
    }
    let mut list = IdList::new(vocab.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if is_word_byte(b) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            match ids.get(&input[i..j]) {
                Some(&vi) => {
                    out.push(TOK_ESC);
                    out.push(list.id(vi));
                    list.touch(vi, mode);
                }
                None => out.extend_from_slice(&input[i..j]),
            }
            i = j;
        } else if b == TOK_ESC {
            out.push(TOK_ESC);
            out.push(0);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`word_token_encode`]. Total on malformed input.
pub fn word_token_decode(data: &[u8]) -> Vec<u8> {
    word_token_decode_mode(data, IdMode::Static)
}

/// Exact inverse of [`word_token_encode_mode`]. Total on malformed input.
pub fn word_token_decode_mode(data: &[u8], mode: IdMode) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let count = data[0] as usize;
    let mut i = 1usize;
    let mut dict: Vec<&[u8]> = Vec::with_capacity(count);
    for _ in 0..count {
        if i >= data.len() {
            break;
        }
        let l = data[i] as usize;
        i += 1;
        if i + l > data.len() {
            break;
        }
        dict.push(&data[i..i + l]);
        i += l;
    }
    let mut list = IdList::new(dict.len());
    let mut out = Vec::new();
    while i < data.len() {
        let b = data[i];
        if b == TOK_ESC {
            if i + 1 < data.len() {
                let id = data[i + 1] as usize;
                i += 2;
                if id == 0 {
                    out.push(0);
                } else if id <= dict.len() {
                    let vi = list.resolve(id as u8);
                    out.extend_from_slice(dict[vi as usize]);
                    list.touch(vi, mode);
                }
            } else {
                i += 1;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

// --- A1.1/A26 v2: escape-extended vocabulary (many more tokens) -------------
//
// v1 addresses only 255 tokens with a two-byte `0x00 id`. v2 keeps two-byte
// tokens for the 254 most common words and adds an escape-extended three-byte id
// for the long tail, so coverage can grow by orders of magnitude:
//
//   dictionary = [u16 count LE][ (u8 len, bytes) * count ]
//   body       = 0x00 0x00            -> literal NUL
//                0x00 id  (1..=254)   -> token id
//                0x00 0xFF hi lo      -> token id = 255 + (hi<<8 | lo)
//                other byte           -> copied verbatim
pub const MAX_TOKENS2: usize = 65534;
const TOK2_EXT: u8 = 0xFF;
/// Ids at or below this are encoded in two bytes; larger ids take three.
const TOK2_TWO_BYTE: usize = 254;

/// Count maximal ASCII-letter runs. Shared by v1 and v2 vocabulary builders.
fn word_counts(input: &[u8]) -> std::collections::HashMap<Vec<u8>, u64> {
    let mut counts: std::collections::HashMap<Vec<u8>, u64> = std::collections::HashMap::new();
    let mut i = 0;
    while i < input.len() {
        if is_word_byte(input[i]) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            *counts.entry(input[i..j].to_vec()).or_insert(0) += 1;
            i = j;
        } else {
            i += 1;
        }
    }
    counts
}

/// Build the v2 vocabulary. Code length is assigned by *kept* rank (the first 254
/// entries get two-byte tokens), and a word is admitted only when its token
/// recovers more than its definition cost at that code length.
pub fn build_word_vocab2(input: &[u8]) -> Vec<Vec<u8>> {
    let counts = word_counts(input);
    let mut cand: Vec<(Vec<u8>, u64)> = counts
        .into_iter()
        .filter(|(w, c)| w.len() >= 3 && *c >= 2)
        .collect();
    cand.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut kept: Vec<Vec<u8>> = Vec::new();
    for (w, c) in cand {
        let len = w.len() as i64;
        let code_len = if kept.len() < TOK2_TWO_BYTE { 2 } else { 3 };
        if c as i64 * (len - code_len) - (len + 1) > 0 {
            kept.push(w);
        }
        if kept.len() >= MAX_TOKENS2 {
            break;
        }
    }
    kept
}

/// Encode `input`, emitting the v2 `dictionary || body`.
pub fn word_token2_encode(input: &[u8]) -> Vec<u8> {
    use std::collections::HashMap;
    let vocab = build_word_vocab2(input);
    let mut ids: HashMap<&[u8], u32> = HashMap::with_capacity(vocab.len());
    let mut out = Vec::with_capacity(input.len());
    out.extend_from_slice(&(vocab.len() as u16).to_le_bytes());
    for (k, w) in vocab.iter().enumerate() {
        out.push(w.len() as u8);
        out.extend_from_slice(w);
        ids.insert(w.as_slice(), (k + 1) as u32);
    }
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if is_word_byte(b) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            match ids.get(&input[i..j]) {
                Some(&id) => {
                    out.push(TOK_ESC);
                    if id <= TOK2_TWO_BYTE as u32 {
                        out.push(id as u8);
                    } else {
                        let v = id - 255;
                        out.push(TOK2_EXT);
                        out.push((v >> 8) as u8);
                        out.push((v & 0xFF) as u8);
                    }
                }
                None => out.extend_from_slice(&input[i..j]),
            }
            i = j;
        } else if b == TOK_ESC {
            out.push(TOK_ESC);
            out.push(0);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`word_token2_encode`]. Total on malformed input.
pub fn word_token2_decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 2 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([data[0], data[1]]) as usize;
    let mut i = 2usize;
    let mut dict: Vec<&[u8]> = Vec::with_capacity(count);
    for _ in 0..count {
        if i >= data.len() {
            break;
        }
        let l = data[i] as usize;
        i += 1;
        if i + l > data.len() {
            break;
        }
        dict.push(&data[i..i + l]);
        i += l;
    }
    let mut out = Vec::new();
    while i < data.len() {
        let b = data[i];
        if b != TOK_ESC {
            out.push(b);
            i += 1;
            continue;
        }
        if i + 1 >= data.len() {
            break;
        }
        let x = data[i + 1];
        if x == 0 {
            out.push(0);
            i += 2;
        } else if x == TOK2_EXT {
            if i + 3 >= data.len() {
                break;
            }
            let id = 255 + (((data[i + 2] as usize) << 8) | data[i + 3] as usize);
            i += 4;
            if id >= 1 && id <= dict.len() {
                out.extend_from_slice(dict[id - 1]);
            }
        } else {
            let id = x as usize;
            i += 2;
            if id <= dict.len() {
                out.extend_from_slice(dict[id - 1]);
            }
        }
    }
    out
}

// --- Phase 4.6: stem / root+affix production transform ----------------------
//
// A reversible morphological transform: a word that ends in a known inflection is
// emitted as `MARK code stem`, and the decoder re-appends the suffix. This is the
// `MorphologicalProduction` of the ledger (`root id + transform`), specialised to
// English suffixes. `code` is a suffix-table index; the `stem` is the remaining
// letters (>= 2 of them), which are copied verbatim so case is preserved.
//
//   literal 0x00 or 0x01 -> 0x00, byte
//   stem word            -> 0x01, code, stem
pub const STEM_ESC: u8 = 0x00;
pub const STEM_MARK: u8 = 0x01;
/// Suffix table, longest-match wins.
pub const SUFFIXES: [&[u8]; 20] = [
    b"ation", b"ition", b"sion", b"tion", b"ness", b"ment", b"able", b"ible", b"less", b"ing",
    b"est", b"ity", b"ive", b"ous", b"ful", b"ly", b"es", b"ed", b"er", b"s",
];

/// Longest suffix of `w` present in the table that leaves a stem of >= 2 bytes.
fn stem_split(w: &[u8]) -> Option<(usize, u8)> {
    let mut best: Option<(usize, u8)> = None;
    for (i, suf) in SUFFIXES.iter().enumerate() {
        let l = suf.len();
        if w.len() >= l + 2 && w.ends_with(suf) {
            match best {
                Some((bl, _)) if bl >= l => {}
                _ => best = Some((l, i as u8)),
            }
        }
    }
    best
}

/// Encode with the stem transform.
pub fn stem_encode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b.is_ascii_alphabetic() {
            let mut j = i;
            while j < input.len() && input[j].is_ascii_alphabetic() {
                j += 1;
            }
            let w = &input[i..j];
            match stem_split(w) {
                Some((l, code)) => {
                    out.push(STEM_MARK);
                    out.push(code);
                    out.extend_from_slice(&w[..w.len() - l]);
                }
                None => out.extend_from_slice(w),
            }
            i = j;
        } else if b <= STEM_MARK {
            out.push(STEM_ESC);
            out.push(b);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`stem_encode`]. Total on malformed input.
pub fn stem_decode(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == STEM_ESC {
            if i + 1 < input.len() {
                out.push(input[i + 1]);
                i += 2;
            } else {
                out.push(STEM_ESC);
                i += 1;
            }
        } else if b == STEM_MARK {
            if i + 1 < input.len() {
                let code = input[i + 1] as usize;
                i += 2;
                let s = i;
                while i < input.len() && input[i].is_ascii_alphabetic() {
                    i += 1;
                }
                out.extend_from_slice(&input[s..i]);
                if code < SUFFIXES.len() {
                    out.extend_from_slice(SUFFIXES[code]);
                }
            } else {
                out.push(STEM_MARK);
                i += 1;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

// --- Phase 4.9: phrase vocabulary (multi-word tokens) -----------------------
//
// Extends the A1.1 word vocabulary to frequent adjacent-word phrases ("w1 w2"
// separated by a single space). The token encoding and the decoder are identical
// to v1: a dictionary entry simply contains a space, so `word_token_decode`
// expands phrase tokens exactly.
pub fn build_phrase_vocab(input: &[u8], reverse: bool) -> Vec<Vec<u8>> {
    use std::collections::HashMap;
    let mut counts: HashMap<Vec<u8>, u64> = HashMap::new();
    let mut i = 0;
    while i < input.len() {
        if is_word_byte(input[i]) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            *counts.entry(input[i..j].to_vec()).or_insert(0) += 1;
            if j + 1 < input.len() && input[j] == b' ' && is_word_byte(input[j + 1]) {
                let mut k = j + 1;
                while k < input.len() && is_word_byte(input[k]) {
                    k += 1;
                }
                let mut ph = Vec::with_capacity(k - i);
                ph.extend_from_slice(&input[i..k]);
                *counts.entry(ph).or_insert(0) += 1;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    let mut cand: Vec<(Vec<u8>, u64)> = counts
        .into_iter()
        .filter(|(w, c)| *c >= 2 && w.len() >= 3)
        .filter(|(w, c)| {
            let len = w.len() as i64;
            let count = *c as i64;
            count * (len - 2) - (len + 1) > 0
        })
        .collect();
    cand.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    cand.truncate(MAX_TOKENS);
    if reverse {
        cand.reverse();
    }
    cand.into_iter().map(|(w, _)| w).collect()
}

/// Encode with the phrase vocabulary; decode with [`word_token_decode`].
pub fn word_token_phrase_encode(input: &[u8], reverse: bool) -> Vec<u8> {
    use std::collections::HashMap;
    let vocab = build_phrase_vocab(input, reverse);
    let mut ids: HashMap<&[u8], u8> = HashMap::with_capacity(vocab.len());
    let mut out = Vec::with_capacity(input.len());
    out.push(vocab.len() as u8);
    for (k, w) in vocab.iter().enumerate() {
        out.push(w.len() as u8);
        out.extend_from_slice(w);
        ids.insert(w.as_slice(), (k + 1) as u8);
    }
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if is_word_byte(b) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            let mut matched = false;
            if j + 1 < input.len() && input[j] == b' ' && is_word_byte(input[j + 1]) {
                let mut k = j + 1;
                while k < input.len() && is_word_byte(input[k]) {
                    k += 1;
                }
                if let Some(&id) = ids.get(&input[i..k]) {
                    out.push(TOK_ESC);
                    out.push(id);
                    i = k;
                    matched = true;
                }
            }
            if !matched {
                match ids.get(&input[i..j]) {
                    Some(&id) => {
                        out.push(TOK_ESC);
                        out.push(id);
                    }
                    None => out.extend_from_slice(&input[i..j]),
                }
                i = j;
            }
        } else if b == TOK_ESC {
            out.push(TOK_ESC);
            out.push(0);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

// --- Phase 4.10: front-coded dictionary ------------------------------------
//
// Same token encoding as v1, but the stored vocabulary is front-coded against
// the previous entry (shared-prefix length + suffix), the Brotli/LZMA header
// idea for shrinking the dictionary the archive must carry (ledger B1).
pub fn word_token_front_encode(input: &[u8], reverse: bool) -> Vec<u8> {
    use std::collections::HashMap;
    let vocab = build_word_vocab(input, reverse);
    let mut ids: HashMap<&[u8], u8> = HashMap::with_capacity(vocab.len());
    let mut out = Vec::with_capacity(input.len());
    out.push(vocab.len() as u8);
    let mut prev: &[u8] = b"";
    for (k, w) in vocab.iter().enumerate() {
        let mut shared = 0usize;
        while shared < prev.len() && shared < w.len() && prev[shared] == w[shared] {
            shared += 1;
        }
        out.push(shared.min(255) as u8);
        let suf = &w[shared..];
        out.push(suf.len() as u8);
        out.extend_from_slice(suf);
        ids.insert(w.as_slice(), (k + 1) as u8);
        prev = w;
    }
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if is_word_byte(b) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            match ids.get(&input[i..j]) {
                Some(&id) => {
                    out.push(TOK_ESC);
                    out.push(id);
                }
                None => out.extend_from_slice(&input[i..j]),
            }
            i = j;
        } else if b == TOK_ESC {
            out.push(TOK_ESC);
            out.push(0);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`word_token_front_encode`]. Total on malformed input.
pub fn word_token_front_decode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let count = data[0] as usize;
    let mut i = 1usize;
    let mut dict: Vec<Vec<u8>> = Vec::with_capacity(count);
    let mut prev: Vec<u8> = Vec::new();
    for _ in 0..count {
        if i + 1 >= data.len() {
            break;
        }
        let shared = (data[i] as usize).min(prev.len());
        let l = data[i + 1] as usize;
        i += 2;
        if i + l > data.len() {
            break;
        }
        let mut w = prev[..shared].to_vec();
        w.extend_from_slice(&data[i..i + l]);
        i += l;
        prev = w.clone();
        dict.push(w);
    }
    let mut out = Vec::new();
    while i < data.len() {
        let b = data[i];
        if b == TOK_ESC {
            if i + 1 < data.len() {
                let id = data[i + 1] as usize;
                i += 2;
                if id == 0 {
                    out.push(0);
                } else if id <= dict.len() {
                    out.extend_from_slice(&dict[id - 1]);
                }
            } else {
                i += 1;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

// --- Phase 4.8: affix-referenced token entries -------------------------------
//
// Brotli-style dictionary transforms: a word not in the vocabulary can still be
// encoded by reference to a vocabulary base plus an affix code, so inflectional
// variants need no stored entry. Id 255 is reserved as the derived marker, so
// direct tokens are 1..=254. `0x00 0xFF base code` expands to `dict[base-1] +
// SUFFIXES[code]`.
pub const AFFIX_MARK: u8 = 0xFF;

pub fn word_token_affix_encode(input: &[u8], reverse: bool) -> Vec<u8> {
    use std::collections::HashMap;
    let mut vocab = build_word_vocab(input, reverse);
    vocab.truncate(254);
    let mut ids: HashMap<&[u8], u8> = HashMap::with_capacity(vocab.len());
    let mut out = Vec::with_capacity(input.len());
    out.push(vocab.len() as u8);
    for (k, w) in vocab.iter().enumerate() {
        out.push(w.len() as u8);
        out.extend_from_slice(w);
        ids.insert(w.as_slice(), (k + 1) as u8);
    }
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if is_word_byte(b) {
            let mut j = i;
            while j < input.len() && is_word_byte(input[j]) {
                j += 1;
            }
            let w = &input[i..j];
            if let Some(&id) = ids.get(w) {
                out.push(TOK_ESC);
                out.push(id);
            } else if let Some((l, code)) = stem_split(w) {
                let stem = &w[..w.len() - l];
                if let Some(&base) = ids.get(stem) {
                    out.push(TOK_ESC);
                    out.push(AFFIX_MARK);
                    out.push(base);
                    out.push(code);
                } else {
                    out.extend_from_slice(w);
                }
            } else {
                out.extend_from_slice(w);
            }
            i = j;
        } else if b == TOK_ESC {
            out.push(TOK_ESC);
            out.push(0);
            i += 1;
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
}

/// Exact inverse of [`word_token_affix_encode`].
pub fn word_token_affix_decode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let count = data[0] as usize;
    let mut i = 1usize;
    let mut dict: Vec<&[u8]> = Vec::with_capacity(count);
    for _ in 0..count {
        if i >= data.len() {
            break;
        }
        let l = data[i] as usize;
        i += 1;
        if i + l > data.len() {
            break;
        }
        dict.push(&data[i..i + l]);
        i += l;
    }
    let mut out = Vec::new();
    while i < data.len() {
        let b = data[i];
        if b == TOK_ESC {
            if i + 1 >= data.len() {
                break;
            }
            let x = data[i + 1];
            if x == 0 {
                out.push(0);
                i += 2;
            } else if x == AFFIX_MARK {
                if i + 3 >= data.len() {
                    break;
                }
                let base = data[i + 2] as usize;
                let code = data[i + 3] as usize;
                i += 4;
                if base >= 1 && base <= dict.len() {
                    out.extend_from_slice(dict[base - 1]);
                    if code < SUFFIXES.len() {
                        out.extend_from_slice(SUFFIXES[code]);
                    }
                }
            } else {
                let id = x as usize;
                i += 2;
                if id <= dict.len() {
                    out.extend_from_slice(dict[id - 1]);
                }
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
    out
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

    // --- A1.2 case factorization -------------------------------------------

    fn case_roundtrip(data: &[u8]) {
        assert_eq!(
            case_decode(&case_encode(data)),
            data,
            "case merge roundtrip"
        );
        assert_eq!(
            case_decode_markonly(&case_encode_markonly(data)),
            data,
            "case mark-only roundtrip"
        );
    }

    /// Deterministic pseudo-random bytes, for property tests without a dev-dep.
    fn xorshift_bytes(seed: u64, len: usize) -> Vec<u8> {
        let mut s = seed | 1;
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                (s >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn case_roundtrip_examples() {
        case_roundtrip(b"");
        case_roundtrip(b"The Quick BROWN fox jumps over the lazy dog.");
        case_roundtrip(b"HelloWorld mixedCASE iPhone McDonald");
        case_roundtrip(b"aA Aa AA aa a A");
        case_roundtrip(b"<page><title>Zentropy</title></page>\n");
    }

    #[test]
    fn case_roundtrip_all_bytes() {
        case_roundtrip(&(0..=255u8).collect::<Vec<u8>>());
    }

    #[test]
    fn case_roundtrip_marker_heavy() {
        // Every case marker and escape byte, adjacent to word and non-word runs.
        for tail in [
            &b"\x00"[..],
            &b"\x01"[..],
            &b"\x02"[..],
            &b"\x03"[..],
            &b"\x00\x01\x02\x03"[..],
            &b"\x01Abc\x02X\x03aB\x00"[..],
            &b"Ab\x01ab\x00cd\x02"[..],
        ] {
            case_roundtrip(tail);
        }
    }

    #[test]
    fn case_roundtrip_random() {
        for seed in 1..=32u64 {
            case_roundtrip(&xorshift_bytes(seed, 4096));
        }
        // A mixture of text-like bytes and full-range noise.
        let mut mix = Vec::new();
        for seed in 1..=8u64 {
            mix.extend_from_slice(b"Some Words Are Capitalized. others are not! ");
            mix.extend_from_slice(&xorshift_bytes(seed * 7, 512));
        }
        case_roundtrip(&mix);
    }

    #[test]
    fn case_merge_folds_identity_and_marks_case() {
        // Title and UPPER words are lowercased behind a one-byte marker, so the
        // lexical identity the predictor sees is a single form.
        assert_eq!(
            case_encode(b"Compression COMPRESSION compression"),
            [
                &[CASE_TITLE][..],
                b"compression ",
                &[CASE_UPPER][..],
                b"compression compression",
            ]
            .concat()
        );
        // Mixed case is preserved verbatim, but still marked so it is exact.
        assert_eq!(
            case_encode(b"iPhone"),
            [&[CASE_MIXED][..], b"iPhone"].concat()
        );
        // Literal control bytes must be escaped, never confused with markers.
        assert_eq!(case_encode(b"\x01"), [CASE_ESC, 0x01]);
    }

    #[test]
    fn case_markonly_does_not_merge_identity() {
        // The control marks case but leaves the word itself untouched, so it
        // isolates marker cost from the value of merging lexical identity.
        assert_eq!(
            case_encode_markonly(b"of Q"),
            [&b"of "[..], &[CASE_TITLE][..], b"Q"].concat()
        );
    }

    // --- A1.1 / A26 dynamic word vocabulary --------------------------------

    fn token_roundtrip(data: &[u8]) {
        for reverse in [false, true] {
            assert_eq!(
                word_token_decode(&word_token_encode(data, reverse)),
                data,
                "word token roundtrip (reverse={reverse})"
            );
        }
    }

    #[test]
    fn token_roundtrip_examples() {
        token_roundtrip(b"");
        token_roundtrip(b"the quick brown fox and the lazy dog");
        token_roundtrip(b"compression compression compression algorithm");
        token_roundtrip(b"<page><title>Zentropy</title></page>\n");
    }

    #[test]
    fn token_roundtrip_all_bytes_and_noise() {
        token_roundtrip(&(0..=255u8).collect::<Vec<u8>>());
        for seed in 1..=32u64 {
            token_roundtrip(&xorshift_bytes(seed, 4096));
        }
        let mut mix = Vec::new();
        for seed in 1..=8u64 {
            mix.extend_from_slice(b"the theory of the compression of the data ");
            mix.extend_from_slice(&xorshift_bytes(seed * 11, 512));
        }
        token_roundtrip(&mix);
    }

    #[test]
    fn token_vocab_is_deterministic_and_gain_ranked() {
        // "the" is the most frequent qualifying word and must rank first.
        let text = b"the cat the dog the bird the fish the the the";
        let v = build_word_vocab(text, false);
        assert_eq!(v.first().map(|w| w.as_slice()), Some(&b"the"[..]));
        // Rebuilding is deterministic regardless of hashing order.
        for _ in 0..8 {
            assert_eq!(build_word_vocab(text, false), v);
        }
        // The control keeps the same word set but reverses the id assignment.
        let mut rev = v.clone();
        rev.reverse();
        assert_eq!(build_word_vocab(text, true), rev);
    }

    #[test]
    fn token_replaces_frequent_word_and_stores_dictionary() {
        let text = b"compression compression compression";
        let enc = word_token_encode(text, false);
        // Shorter than the input despite carrying the dictionary: the three
        // occurrences collapse to two bytes each.
        assert!(enc.len() < text.len(), "token stream did not shrink");
        assert_eq!(word_token_decode(&enc), text);
    }

    #[test]
    fn token_escapes_literal_nul() {
        // A literal 0x00 must never be confused with a token prefix, and id 0
        // must never be a valid token id.
        let text = b"a\x00b a\x00b a\x00b";
        assert_eq!(word_token_decode(&word_token_encode(text, false)), text);
    }

    // --- Phase 10.4: recency-ranked token ids -------------------------------

    /// Phase 10.5: the first-use form must round-trip, and its size must match the
    /// arithmetic the design claims — exactly `+1` byte per entry (a definition
    /// costs `escape + id + len + word`, the header entry it replaces costs
    /// `len + word`), less the header's one-byte count. Asserting the byte
    /// identity catches a format slip that a round-trip alone would not
    /// (a symmetric bug in encode and decode still round-trips).
    #[test]
    fn firstuse_definitions_roundtrip_and_cost_one_byte_per_entry() {
        let text = b"compression compression algorithm compression algorithm algorithm \
                     galaxy compression galaxy algorithm";
        let vocab = build_word_vocab(text, true);
        assert!(!vocab.is_empty(), "test needs a non-empty vocabulary");

        let header = word_token_encode_vocab(text, &vocab, IdMode::Static);
        let first = word_token_encode_firstuse(text, &vocab);
        assert_eq!(
            word_token_decode_firstuse(&first),
            text,
            "first-use not exact"
        );
        assert_eq!(word_token_decode(&header), text, "header form not exact");

        // Every entry is used (the builder requires count >= 2 and the vocabulary
        // is derived from this very text), so the identity holds exactly.
        assert_eq!(
            first.len(),
            header.len() + vocab.len() - 1,
            "first-use cost is not +1 byte per entry less the count byte \
             (vocab {}, header {}, first-use {})",
            vocab.len(),
            header.len(),
            first.len()
        );
    }

    #[test]
    fn firstuse_caps_ids_below_the_definition_escape() {
        // 0xFF is the definition marker, so it can never be a token id. A
        // vocabulary longer than the cap must be truncated rather than producing a
        // stream the decoder would misread.
        assert!(FIRST_USE_MAX_TOKENS < 255);
        let long: Vec<Vec<u8>> = (0..300u32).map(|i| format!("w{i}x").into_bytes()).collect();
        let text = b"w1x w1x w2x w2x";
        let enc = word_token_encode_firstuse(text, &long);
        assert_eq!(word_token_decode_firstuse(&enc), text);
    }

    /// Phase 10: encoding against an explicit vocabulary must round-trip through
    /// the normal decoder, which knows nothing about how the vocabulary was
    /// chosen. That is the whole safety argument for the priced-vocabulary
    /// experiment, so it is asserted directly rather than assumed.
    #[test]
    fn explicit_vocabulary_roundtrips() {
        let text = b"alpha beta gamma alpha beta gamma compression compression";
        for vocab in [
            vec![b"alpha".to_vec(), b"beta".to_vec()],
            vec![b"gamma".to_vec()],
            vec![b"compression".to_vec(), b"alpha".to_vec(), b"beta".to_vec()],
            Vec::new(),
        ] {
            let enc = word_token_encode_vocab(text, &vocab, IdMode::Static);
            assert_eq!(word_token_decode(&enc), text, "vocab {vocab:?} failed");
            let ids = id_stream_of(&enc);
            // Only the listed words may become tokens.
            assert!(
                ids.len()
                    <= vocab
                        .iter()
                        .filter(|w| text.windows(w.len()).count() > 0)
                        .count()
                        * 3,
                "unexpected token count for {vocab:?}"
            );
        }
    }

    /// The id stream of a v1 token stream, skipping the dictionary header.
    fn id_stream_of(enc: &[u8]) -> Vec<u8> {
        let count = enc[0] as usize;
        let mut i = 1usize;
        for _ in 0..count {
            let l = enc[i] as usize;
            i += 1 + l;
        }
        let mut v = Vec::new();
        while i < enc.len() {
            if enc[i] == TOK_ESC {
                v.push(enc[i + 1]);
                i += 2;
            } else {
                i += 1;
            }
        }
        v
    }

    /// The identity control that makes the id-order experiment measurable: the
    /// static mode must be byte-identical to the shipped v1 encoder, so any
    /// difference in the archive is attributable to the id *assignment* and not
    /// to a reimplementation of the tokenizer.
    #[test]
    fn id_mode_static_is_bit_identical_to_the_shipped_tokenizer() {
        for text in [
            &b""[..],
            &b"the quick brown fox and the lazy dog"[..],
            &b"compression compression compression algorithm algorithm"[..],
            &b"<page><title>Zentropy</title><text>archive archive</text></page>\n"[..],
            &b"a\x00b a\x00b"[..],
        ] {
            for reverse in [false, true] {
                assert_eq!(
                    word_token_encode_mode(text, reverse, IdMode::Static),
                    word_token_encode(text, reverse),
                    "static mode diverged for {text:?} reverse={reverse}"
                );
            }
        }
    }

    #[test]
    fn id_modes_roundtrip_exactly() {
        let cases: [&[u8]; 6] = [
            b"",
            b"the quick brown fox and the lazy dog",
            b"compression compression compression algorithm algorithm algorithm",
            b"alpha beta alpha beta alpha beta",
            b"<page><title>Zentropy</title><text>archive archive</text></page>\n",
            b"a\x00b a\x00b a\x00b",
        ];
        for text in cases {
            for reverse in [false, true] {
                for mode in [IdMode::Static, IdMode::Mtf, IdMode::MoveToSecond] {
                    let enc = word_token_encode_mode(text, reverse, mode);
                    assert_eq!(
                        word_token_decode_mode(&enc, mode),
                        text,
                        "mode {mode:?} reverse={reverse} failed on {text:?}"
                    );
                }
            }
        }
    }

    /// Move-to-front must actually move to front, and the two modes must differ.
    /// Asserting the id stream directly is stronger than inferring the mechanism
    /// from a size: a mode that never promoted would still round-trip.
    #[test]
    fn mtf_promotes_the_most_recent_token() {
        // The vocabulary keeps a word only when `count * (len - 2) > len + 1`, so
        // alpha (len 5) needs count >= 3 and beta (len 4) needs count >= 3. A
        // first attempt used each word twice, which left only `alpha` as a token
        // and tested nothing about promotion.
        let text = b"alpha beta beta beta gamma gamma gamma alpha alpha alpha";

        // The id bytes of the body, in order, skipping the vocabulary header.
        fn id_stream(enc: &[u8]) -> Vec<u8> {
            id_stream_of(enc)
        }

        let mtf = word_token_encode_mode(text, false, IdMode::Mtf);
        assert_eq!(word_token_decode_mode(&mtf, IdMode::Mtf), text);
        let ids = id_stream(&mtf);
        assert_eq!(ids.len(), 10, "expected ten token emissions, got {ids:?}");
        // alpha, beta, beta, beta, gamma, gamma, gamma, alpha, alpha, alpha.
        // Every immediate repeat is the most recently used token, so it must be
        // id 1: positions 2, 3 (beta), 5, 6 (gamma) and 8, 9 (alpha).
        for k in [2usize, 3, 5, 6, 8, 9] {
            assert_eq!(
                ids[k], 1,
                "immediate repeat at {k} was not promoted: {ids:?}"
            );
        }
        // At least one id must exceed 1, or the mode is collapsing everything to
        // one id and the promotion is not doing anything selective.
        assert!(
            ids.iter().any(|&i| i > 1),
            "all ids collapsed to 1: {ids:?}"
        );
        assert!(
            ids.iter().all(|&i| (1..=3).contains(&i)),
            "id out of range: {ids:?}"
        );

        let m2 = word_token_encode_mode(text, false, IdMode::MoveToSecond);
        assert_eq!(word_token_decode_mode(&m2, IdMode::MoveToSecond), text);
        assert_ne!(ids, id_stream(&m2), "the two modes produced identical ids");

        // The static control must be a different stream again, or the experiment
        // would have nothing to attribute a delta to.
        let st = word_token_encode_mode(text, false, IdMode::Static);
        assert_ne!(ids, id_stream(&st));
    }

    // --- A1.1/A26 v2 escape-extended vocabulary -----------------------------

    fn token2_roundtrip(data: &[u8]) {
        assert_eq!(word_token2_decode(&word_token2_encode(data)), data);
    }

    #[test]
    fn token2_roundtrip_examples() {
        token2_roundtrip(b"");
        token2_roundtrip(b"the quick brown fox and the lazy dog");
        token2_roundtrip(b"<page><title>Zentropy</title></page>\n");
    }

    #[test]
    fn token2_roundtrip_all_bytes_and_noise() {
        token2_roundtrip(&(0..=255u8).collect::<Vec<u8>>());
        for seed in 1..=32u64 {
            token2_roundtrip(&xorshift_bytes(seed, 4096));
        }
        let mut mix = Vec::new();
        for seed in 1..=8u64 {
            mix.extend_from_slice(b"the theory of the compression of the data ");
            mix.extend_from_slice(&xorshift_bytes(seed * 13, 512));
        }
        token2_roundtrip(&mix);
    }

    // --- Phase 4.6 stem transform -----------------------------------------

    fn stem_roundtrip(data: &[u8]) {
        assert_eq!(stem_decode(&stem_encode(data)), data, "stem roundtrip");
    }

    #[test]
    fn stem_roundtrip_examples() {
        stem_roundtrip(b"");
        stem_roundtrip(b"compression rendering responsibilities running cats");
        stem_roundtrip(b"a s ed ing the and");
        stem_roundtrip(b"\x00\x01\x01ab\x00");
    }

    #[test]
    fn stem_roundtrip_all_bytes_and_random() {
        stem_roundtrip(&(0..=255u8).collect::<Vec<u8>>());
        for seed in 1..=32u64 {
            stem_roundtrip(&xorshift_bytes(seed, 4096));
        }
    }

    #[test]
    fn stem_produces_stem_plus_affix() {
        // "rendering" -> mark, code(ing), "render"
        let e = stem_encode(b"rendering");
        assert_eq!(e[0], STEM_MARK);
        assert_eq!(
            e[1] as usize,
            SUFFIXES.iter().position(|s| *s == b"ing").unwrap()
        );
        assert_eq!(&e[2..], b"render");
        assert_eq!(stem_decode(&e), b"rendering");
    }

    // --- Phase 4.9 phrase vocabulary --------------------------------------

    fn phrase_roundtrip(data: &[u8]) {
        for rev in [false, true] {
            assert_eq!(
                word_token_decode(&word_token_phrase_encode(data, rev)),
                data,
                "phrase roundtrip"
            );
        }
    }

    #[test]
    fn phrase_roundtrip_examples() {
        phrase_roundtrip(b"");
        phrase_roundtrip(b"the united states of america and the united nations");
        phrase_roundtrip(b"\x00\x01 the and");
    }

    #[test]
    fn phrase_roundtrip_all_bytes_and_random() {
        phrase_roundtrip(&(0..=255u8).collect::<Vec<u8>>());
        for seed in 1..=16u64 {
            phrase_roundtrip(&xorshift_bytes(seed, 4096));
        }
    }

    #[test]
    fn phrase_vocab_includes_a_bigram() {
        let text = b"united states united states united states united states";
        let v = build_phrase_vocab(text, false);
        assert!(
            v.iter().any(|w| w.as_slice() == b"united states"),
            "phrase not learned: {v:?}"
        );
    }

    // --- Phase 4.10 front-coded dictionary --------------------------------

    fn front_roundtrip(data: &[u8]) {
        for rev in [false, true] {
            assert_eq!(
                word_token_front_decode(&word_token_front_encode(data, rev)),
                data,
                "front roundtrip"
            );
        }
    }

    #[test]
    fn front_roundtrip_examples() {
        front_roundtrip(b"");
        front_roundtrip(b"the quick brown fox and the lazy dog");
        front_roundtrip(b"\x00\x01 the and");
    }

    #[test]
    fn front_roundtrip_all_bytes_and_random() {
        front_roundtrip(&(0..=255u8).collect::<Vec<u8>>());
        for seed in 1..=16u64 {
            front_roundtrip(&xorshift_bytes(seed, 4096));
        }
    }

    // --- Phase 4.8 affix-referenced entries -------------------------------

    fn affix_roundtrip(data: &[u8]) {
        for rev in [false, true] {
            assert_eq!(
                word_token_affix_decode(&word_token_affix_encode(data, rev)),
                data,
                "affix roundtrip"
            );
        }
    }

    #[test]
    fn affix_roundtrip_examples() {
        affix_roundtrip(b"");
        affix_roundtrip(b"rendering render rendering renders render");
        affix_roundtrip(b"\x00\x01 the and");
    }

    #[test]
    fn affix_roundtrip_all_bytes_and_random() {
        affix_roundtrip(&(0..=255u8).collect::<Vec<u8>>());
        for seed in 1..=16u64 {
            affix_roundtrip(&xorshift_bytes(seed, 4096));
        }
    }

    #[test]
    fn token2_exercises_three_byte_ids() {
        // 600 distinct five-letter words, each repeated 8 times, force the
        // vocabulary past the 254 two-byte-id budget so three-byte tokens are
        // actually emitted and decoded.
        let mut text = Vec::new();
        for i in 0..600u32 {
            let w = [
                b'a' + (i % 26) as u8,
                b'a' + ((i / 26) % 26) as u8,
                b'a' + ((i / 676) % 26) as u8,
                b'a' + ((i / 17576) % 26) as u8,
                b'a' + ((i / 456976) % 26) as u8,
            ];
            for _ in 0..8 {
                text.extend_from_slice(&w);
                text.push(b' ');
            }
        }
        let vocab = build_word_vocab2(&text);
        assert!(
            vocab.len() > TOK2_TWO_BYTE,
            "vocab did not exceed the two-byte budget"
        );
        token2_roundtrip(&text);
    }
}
