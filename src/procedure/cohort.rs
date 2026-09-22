//! Cohort discovery (§14.13) from *cheap corpus-native signatures*, deterministic.
//!
//! Cohorting is the step that decides which targets are allowed to share one
//! program. It must be:
//!
//! * **corpus-native** — the signature is read off the span's own bytes, not off a
//!   model that would itself have to be paid for;
//! * **deterministic** — the same corpus and class always yield the same cohorts,
//!   in the same order, so a measurement is reproducible;
//! * **cheap** — the signature is a few bytes of structure, not a search.
//!
//! ## Minimum useful cohort size (`DEFAULT_MIN_COHORT`)
//!
//! A cohort with one member cannot share anything: the "shared" program is paid
//! once and used once, which is strictly worse than pricing the member directly.
//! The first-boundary pass measured this directly — 465 `template` spans collapsed
//! into cohorts dominated by singletons, and each singleton paid a whole program
//! (14,097 B of a 24,361 B candidate cost came from the program term). A cohort is
//! therefore marked [`shared`](Cohort::shared) only when it reaches
//! [`DEFAULT_MIN_COHORT`] members (configurable; default 3); below that the
//! skeleton layer charges the members directly and pays **no** program. See
//! [`super::skeleton::synthesise_cohort`] for the fallback itself.
//!
//! ## Purer signatures (§14.13)
//!
//! The first-boundary pass used the *whole* first-level name set for `template`
//! and the *whole* IR `Kind` sequence for everything else. Both are too coarse:
//! two spans that share a `Kind` shape may share no bytes at all, and a template
//! name set that ignores parameters groups `{{cite|url=…}}` with
//! `{{cite|title=…}}`. This module now computes, per class:
//!
//! * `template` — the sorted set of first-level template names **and** the sorted
//!   set of top-level parameter names (not the whole span text);
//! * `wiki_table` — the column count and the per-column *type shape*;
//! * `wiki_link` — the namespace and the target-shape class;
//! * `xml_open` — the tag name plus the sorted attribute-name set;
//! * the generic fallback — the IR `Kind` sequence.
//!
//! A signature is only better if it makes cohorts *tighter*, so the module also
//! reports within-cohort versus between-cohort member similarity
//! ([`stats`]/[`CohortStats`]) rather than asserting the improvement. The old
//! signatures are kept as [`signature_v1`] so the two can be measured side by side.
//!
//! The signature bytes are hashed (FNV-1a, our own, so it cannot drift with a
//! standard-library change) to a 64-bit cohort id. Cohorts are ordered by
//! signature bytes, members by corpus position, so no hashing order leaks into
//! the output.
//!
//! `randomised` is the §14.44 negative control: it keeps the exact cohort *size
//! distribution* but assigns members to cohorts uniformly at random from a
//! seeded, deterministic PRNG. A real cohorting gain must beat it.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir;

use super::extract::{ClassName, Span};

/// The default minimum useful cohort size. Below this a cohort is charged
/// directly and pays no program (see the module docs). Configurable through
/// [`discover_with_min`] and [`super::skeleton::synthesise_with_min`].
pub const DEFAULT_MIN_COHORT: usize = 3;

/// FNV-1a (64-bit), implemented here so the cohort id is a property of this
/// module rather than of the standard library's `Hasher` version.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let low = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(low);
            return;
        }
        out.push(low | 0x80);
    }
}

/// Length-prefix a token list so the encoding is self-delimiting and cannot be
/// confused by a separator byte that happens to occur inside a name.
fn enc_tokens(out: &mut Vec<u8>, tokens: &[Vec<u8>]) {
    put_uvarint(out, tokens.len() as u64);
    for t in tokens {
        put_uvarint(out, t.len() as u64);
        out.extend_from_slice(t);
    }
}

/// The sequence of IR `Kind`s the span tokenises to — the generic "shape".
pub fn shape(span: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    for t in ir::tokenize(span) {
        v.push(t.kind as u8);
    }
    v
}

fn trim_end(mut b: &[u8]) -> &[u8] {
    while let Some(&last) = b.last() {
        if last == b' ' || last == b'\t' || last == b'\r' || last == b'\n' {
            b = &b[..b.len() - 1];
        } else {
            break;
        }
    }
    b
}

fn trim_start(mut b: &[u8]) -> &[u8] {
    while let Some(&first) = b.first() {
        if first == b' ' || first == b'\t' || first == b'\r' {
            b = &b[1..];
        } else {
            break;
        }
    }
    b
}

/// The first-level (brace-depth 0) template names in `span`, sorted and unique.
///
/// Only top-level `{{name...` openings are captured: a nested `{{...}}` inside a
/// parameter is part of its parent's value, not a name of this span. Unbalanced
/// braces degrade to "whatever names were seen before the imbalance", which is
/// still a deterministic function of the bytes.
pub fn first_level_names(span: &[u8]) -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = Vec::new();
    let n = span.len();
    if n < 2 {
        return names;
    }
    let mut i = 0usize;
    let mut depth: i32 = 0;
    while i + 1 < n {
        if span[i] == b'{' && span[i + 1] == b'{' {
            if depth == 0 {
                let mut j = i + 2;
                while j < n && (span[j] == b' ' || span[j] == b'\t' || span[j] == b'\n') {
                    j += 1;
                }
                let mut k = j;
                while k < n && span[k] != b'|' && span[k] != b'}' && span[k] != b'\n' {
                    k += 1;
                }
                let name = trim_end(&span[j..k]);
                if !name.is_empty() {
                    names.push(name.to_vec());
                }
            }
            depth += 1;
            i += 2;
            continue;
        }
        if span[i] == b'}' && span[i + 1] == b'}' {
            if depth > 0 {
                depth -= 1;
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    names.sort();
    names.dedup();
    names
}

/// The first-level (brace-depth 0) **named** parameter names in `span`, sorted
/// and unique.
///
/// A parameter is named when it is written `|name=value`. Positional parameters
/// (`|value`) have no name and are deliberately *not* captured: their bytes are
/// the innovation a residual or slot carries, and a signature that encoded them
/// would put `{{cite|1}}` and `{{cite|2}}` in different cohorts. Only pipes at
/// the top level (depth 1, i.e. directly inside a top-level `{{`) are considered,
/// so a nested template's parameters are its own.
pub fn first_level_params(span: &[u8]) -> Vec<Vec<u8>> {
    let mut params: Vec<Vec<u8>> = Vec::new();
    let n = span.len();
    let mut i = 0usize;
    let mut depth: i32 = 0;
    while i + 1 < n {
        if span[i] == b'{' && span[i + 1] == b'{' {
            depth += 1;
            i += 2;
            continue;
        }
        if span[i] == b'}' && span[i + 1] == b'}' {
            if depth > 0 {
                depth -= 1;
            }
            i += 2;
            continue;
        }
        if depth == 1 && span[i] == b'|' {
            // A parameter name runs from just after the pipe to the first `=`,
            // `|`, `}` or newline. Only a name that is *followed by* `=` counts.
            let mut j = i + 1;
            while j < n && span[j] != b'=' && span[j] != b'|' && span[j] != b'}' && span[j] != b'\n'
            {
                j += 1;
            }
            if j < n && span[j] == b'=' {
                let name = trim_end(&span[i + 1..j]);
                if !name.is_empty() {
                    params.push(name.to_vec());
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    params.sort();
    params.dedup();
    params
}

/// The tag name of an XML open tag, or `None` if the span is not one.
pub fn tag_name(span: &[u8]) -> Option<Vec<u8>> {
    if span.first() != Some(&b'<') {
        return None;
    }
    let rest = &span[1..];
    match rest.first() {
        Some(&b) if b == b'/' || b == b'!' || b == b'?' => return None,
        None => return None,
        _ => {}
    }
    let mut k = 0usize;
    while k < rest.len() && !rest[k].is_ascii_whitespace() && rest[k] != b'>' && rest[k] != b'/' {
        k += 1;
    }
    if k == 0 {
        None
    } else {
        Some(rest[..k].to_vec())
    }
}

/// The sorted, unique attribute **names** of an XML open tag.
///
/// Names are the identifiers before an `=`; values are ignored because they are
/// the innovation. Quotes are respected so a `>` inside a quoted value does not
/// terminate the tag early, and only the first tag is scanned.
pub fn attribute_names(span: &[u8]) -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = Vec::new();
    let n = span.len();
    if span.first() != Some(&b'<') || n < 2 {
        return names;
    }
    let mut i = 1usize;
    // Skip a closing slash for the (unused here) close form; `tag_name` already
    // rejects those, but the scan is defensive.
    if i < n && span[i] == b'/' {
        i += 1;
    }
    // Skip the tag name.
    while i < n && !span[i].is_ascii_whitespace() && span[i] != b'>' && span[i] != b'/' {
        i += 1;
    }
    while i < n {
        let b = span[i];
        if b == b'>' || b == b'/' {
            break;
        }
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // Read a candidate name up to `=`, whitespace, `>`, `/`.
        let start = i;
        while i < n
            && span[i] != b'='
            && !span[i].is_ascii_whitespace()
            && span[i] != b'>'
            && span[i] != b'/'
        {
            i += 1;
        }
        // It is an attribute name only if an `=` follows (optionally after
        // spaces). A bare token is not an attribute.
        let mut j = i;
        while j < n && span[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < n && span[j] == b'=' {
            let name = trim_end(&span[start..i]);
            if !name.is_empty() {
                names.push(name.to_vec());
            }
            // Skip `=` and the value, honouring quotes.
            i = j + 1;
            while i < n && span[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < n && (span[i] == b'"' || span[i] == b'\'') {
                let q = span[i];
                i += 1;
                while i < n && span[i] != q {
                    i += 1;
                }
                if i < n {
                    i += 1;
                }
            } else {
                while i < n && !span[i].is_ascii_whitespace() && span[i] != b'>' {
                    i += 1;
                }
            }
        } else if i == start {
            // Defensive: no progress would loop forever.
            i += 1;
        }
    }
    names.sort();
    names.dedup();
    names
}

/// A one-byte type class for a table cell. The classes are structural, not
/// semantic: an attribute-only cell, a numeric cell, a cell carrying a template,
/// a link, a URL, or plain text. The point is a *stable, cheap* column shape, not
/// a taxonomy.
fn cell_type(cell: &[u8]) -> u8 {
    let c = trim_start(cell);
    if c.is_empty() {
        return 0;
    }
    // Attribute-only cell: `style="…" | data`.
    const ATTRS: [&[u8]; 12] = [
        b"style", b"class", b"align", b"bgcolor", b"rowspan", b"colspan", b"scope", b"id",
        b"title", b"width", b"height", b"valign",
    ];
    for a in ATTRS {
        if c.starts_with(a) && c.get(a.len()) == Some(&b'=') {
            return 1;
        }
    }
    if contains(c, b"{{") {
        return 2;
    }
    if contains(c, b"[[") {
        return 3;
    }
    if contains(c, b"http") {
        return 4;
    }
    if !c.is_empty()
        && c.iter()
            .all(|&b| b.is_ascii_digit() || b == b'.' || b == b',' || b == b'-' || b == b' ')
    {
        return 5;
    }
    6
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Split a table body into rows of cells. Deterministic, line-oriented, and
/// deliberately simple: wikitext tables put one row per `|-` line and cells after
/// a `|` (data) or `!` (header), separated by `||`/`!!` on the same line.
fn table_rows(span: &[u8]) -> Vec<Vec<Vec<u8>>> {
    let mut rows: Vec<Vec<Vec<u8>>> = Vec::new();
    for raw_line in span.split(|&b| b == b'\n') {
        let line = trim_start(raw_line);
        if line.is_empty() {
            continue;
        }
        if line.starts_with(b"{|") || line.starts_with(b"|}") || line.starts_with(b"|-") {
            continue;
        }
        if line.starts_with(b"|+") {
            // Caption: it is not a column-bearing row.
            continue;
        }
        let sep: &[u8] = if line[0] == b'!' {
            b"!!"
        } else if line[0] == b'|' {
            b"||"
        } else {
            continue;
        };
        let rest = &line[1..];
        let mut cells: Vec<Vec<u8>> = Vec::new();
        for part in split_on(rest, sep) {
            cells.push(part.to_vec());
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
    }
    rows
}

fn split_on<'a>(hay: &'a [u8], sep: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i + sep.len() <= hay.len() {
        if &hay[i..i + sep.len()] == sep {
            out.push(&hay[start..i]);
            i += sep.len();
            start = i;
        } else {
            i += 1;
        }
    }
    out.push(&hay[start..]);
    out
}

/// The `wiki_table` signature: column count plus the per-column type shape of the
/// first column-bearing row. `[cols, t0, t1, …]`.
pub fn table_shape(span: &[u8]) -> Vec<u8> {
    let rows = table_rows(span);
    let cols = rows.first().map(|r| r.len()).unwrap_or(0).min(255);
    let mut out = Vec::with_capacity(cols + 1);
    out.push(cols as u8);
    if let Some(first) = rows.first() {
        for c in first.iter().take(cols) {
            out.push(cell_type(c));
        }
    }
    out
}

/// The `wiki_link` signature: the namespace (if the target has one) plus a
/// target-shape flag word. Flags: `1` leading `:` (link suppression),
/// `2` `#` section link, `4` display text (`|`), `8` subpage `/`,
/// `16` target contains a space, `32`/`64`/`128` first target byte is
/// upper/lower/digit.
pub fn link_signature(span: &[u8]) -> Vec<u8> {
    let inner = strip(span, b"[[", b"]]");
    let target = match inner.iter().position(|&b| b == b'|') {
        Some(p) => &inner[..p],
        None => inner,
    };
    // Namespace: `name:` where `name` is short and alphabetic.
    let mut ns: &[u8] = b"";
    let mut rest: &[u8] = target;
    if let Some(ci) = target.iter().position(|&b| b == b':') {
        let head = &target[..ci];
        if !head.is_empty() && head.len() <= 12 && head.iter().all(|b| b.is_ascii_alphabetic()) {
            ns = head;
            rest = &target[ci + 1..];
        }
    }
    let mut lower_ns: Vec<u8> = ns.to_vec();
    lower_ns.make_ascii_lowercase();

    let mut flags: u64 = 0;
    if target.first() == Some(&b':') {
        flags |= 1;
    }
    if target.contains(&b'#') {
        flags |= 2;
    }
    if inner.contains(&b'|') {
        flags |= 4;
    }
    if rest.contains(&b'/') {
        flags |= 8;
    }
    if rest.contains(&b' ') {
        flags |= 16;
    }
    match rest.first() {
        Some(b) if b.is_ascii_uppercase() => flags |= 32,
        Some(b) if b.is_ascii_lowercase() => flags |= 64,
        Some(b) if b.is_ascii_digit() => flags |= 128,
        _ => {}
    }

    let mut out = Vec::new();
    put_uvarint(&mut out, lower_ns.len() as u64);
    out.extend_from_slice(&lower_ns);
    put_uvarint(&mut out, flags);
    out
}

/// `span` with a leading `open` and a trailing `close` removed if both present.
fn strip<'a>(span: &'a [u8], open: &[u8], close: &[u8]) -> &'a [u8] {
    let mut s = span;
    if s.starts_with(open) {
        s = &s[open.len()..];
    }
    if s.ends_with(close) && s.len() >= close.len() {
        s = &s[..s.len() - close.len()];
    }
    s
}

/// The cohort signature of a span for a given class (the §14.13 "purer"
/// signatures).
pub fn signature(class: ClassName, span: &[u8]) -> Vec<u8> {
    match class {
        ClassName::Template => {
            let names = first_level_names(span);
            let params = first_level_params(span);
            if names.is_empty() && params.is_empty() {
                shape(span)
            } else {
                let mut v = Vec::new();
                enc_tokens(&mut v, &names);
                enc_tokens(&mut v, &params);
                v
            }
        }
        ClassName::WikiLink => link_signature(span),
        ClassName::WikiTable => table_shape(span),
        ClassName::XmlOpen => {
            let mut v = Vec::new();
            match tag_name(span) {
                Some(name) => {
                    enc_tokens(&mut v, std::slice::from_ref(&name));
                    enc_tokens(&mut v, &attribute_names(span));
                    v
                }
                None => shape(span),
            }
        }
        _ => shape(span),
    }
}

/// The pre-§14.13 signature, kept so the purer signatures can be measured against
/// it: `template` = name set only; `xml_open` = tag name only; everything else =
/// the IR `Kind` sequence.
#[allow(dead_code)]
pub fn signature_v1(class: ClassName, span: &[u8]) -> Vec<u8> {
    match class {
        ClassName::Template => {
            let names = first_level_names(span);
            if names.is_empty() {
                shape(span)
            } else {
                let mut v = Vec::new();
                for (i, name) in names.iter().enumerate() {
                    if i > 0 {
                        v.push(0x1f);
                    }
                    v.extend_from_slice(name);
                }
                v
            }
        }
        ClassName::XmlOpen => tag_name(span).unwrap_or_else(|| shape(span)),
        _ => shape(span),
    }
}

/// A discovered cohort: an id (the signature hash), member indices into the
/// span list in corpus order, and whether the cohort is large enough to be
/// worth sharing a program ([`DEFAULT_MIN_COHORT`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cohort {
    pub id: u64,
    pub members: Vec<usize>,
    /// `true` iff `members.len() >= min_cohort`: only then may this cohort pay a
    /// shared program. A below-minimum cohort is charged directly (see
    /// [`super::skeleton`]).
    pub shared: bool,
}

fn group<F>(data: &[u8], spans: &[Span], min_cohort: usize, sig_of: F) -> Vec<Cohort>
where
    F: Fn(&[u8]) -> Vec<u8>,
{
    let mut groups: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (i, s) in spans.iter().enumerate() {
        let sig = sig_of(&data[s.start..s.end()]);
        groups.entry(sig).or_default().push(i);
    }
    groups
        .into_iter()
        .map(|(sig, mut members)| {
            members.sort_unstable();
            let shared = members.len() >= min_cohort.max(1);
            Cohort {
                id: fnv1a(&sig),
                members,
                shared,
            }
        })
        .collect()
}

/// Group spans into cohorts by the §14.13 signature at the default minimum size.
/// Cohorts are ordered by signature bytes; members by corpus position.
/// Deterministic by construction.
pub fn discover(data: &[u8], spans: &[Span], class: ClassName) -> Vec<Cohort> {
    discover_with_min(data, spans, class, DEFAULT_MIN_COHORT)
}

/// As [`discover`], at an explicit minimum useful cohort size. A cohort below
/// `min_cohort` is marked `shared = false` and must not be charged a program.
pub fn discover_with_min(
    data: &[u8],
    spans: &[Span],
    class: ClassName,
    min_cohort: usize,
) -> Vec<Cohort> {
    group(data, spans, min_cohort, |s| signature(class, s))
}

/// As [`discover`], using the pre-§14.13 signatures at an explicit minimum size.
/// Exists only so [`stats`] and [`stats_v1`] can be compared on the same corpus.
#[allow(dead_code)]
pub fn discover_v1_with_min(
    data: &[u8],
    spans: &[Span],
    class: ClassName,
    min_cohort: usize,
) -> Vec<Cohort> {
    group(data, spans, min_cohort, |s| signature_v1(class, s))
}

/// A seeded xorshift64* generator. Deterministic, integer-only, and independent
/// of the standard library, so the random-cohort control reproduces exactly.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // The generator is undefined at zero state; any odd seed is a fixed point
        // free of that degenerate case.
        Rng { state: seed | 1 }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform in `0..n` (n > 0). Integer-only.
    pub fn below(&mut self, n: usize) -> usize {
        if n <= 1 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}

/// The §14.44 negative control: same cohort *size distribution*, random member
/// assignment. The cohort ids are re-derived from the shuffled membership so the
/// report cannot accidentally attribute a real id to a random group.
pub fn randomised(cohorts: &[Cohort], rng: &mut Rng) -> Vec<Cohort> {
    let mut pool: Vec<usize> = Vec::new();
    for c in cohorts {
        pool.extend_from_slice(&c.members);
    }
    // Fisher-Yates, deterministic given the seed.
    let n = pool.len();
    for i in (1..n).rev() {
        let j = rng.below(i + 1);
        pool.swap(i, j);
    }
    let mut out = Vec::with_capacity(cohorts.len());
    let mut at = 0usize;
    for c in cohorts {
        let k = c.members.len();
        let mut members: Vec<usize> = pool[at..at + k].to_vec();
        members.sort_unstable();
        at += k;
        // Id derived from the (random) membership, not copied from the real cohort.
        let mut tag = Vec::with_capacity(members.len() * 4);
        for &m in &members {
            tag.extend_from_slice(&(m as u32).to_le_bytes());
        }
        out.push(Cohort {
            id: fnv1a(&tag),
            shared: c.shared,
            members,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Cohesion measurement: is a signature actually making cohorts tighter?
// ---------------------------------------------------------------------------

/// The set of 4-byte-gram hashes of a member, the cheap byte-level fingerprint on
/// which pairwise similarity is computed. Deterministic (FNV-1a); integer-only.
#[allow(dead_code)]
fn grams(bytes: &[u8]) -> BTreeSet<u64> {
    let mut set = BTreeSet::new();
    if bytes.len() < 4 {
        set.insert(fnv1a(bytes));
        return set;
    }
    for w in bytes.windows(4) {
        set.insert(fnv1a(w));
    }
    set
}

/// Jaccard similarity of two gram sets, scaled to permille (0..=1000). Integer
/// arithmetic only; `f64` is deliberately avoided so the measurement is exactly
/// reproducible.
#[allow(dead_code)]
fn jaccard_permille(a: &BTreeSet<u64>, b: &BTreeSet<u64>) -> u64 {
    let inter = a.intersection(b).count() as u128;
    let union = (a.len() + b.len()) as u128 - inter;
    if union == 0 {
        return 1000;
    }
    ((inter * 1000) / union) as u64
}

/// Deterministic sampling stride: keep pair work bounded without letting the
/// choice depend on anything but the cohort size.
#[allow(dead_code)]
fn stride(n: usize, cap: usize) -> usize {
    if n <= cap {
        1
    } else {
        n / cap + 1
    }
}

/// Cohort statistics: size distribution and, crucially, whether the members a
/// signature groups together are actually similar to one another. `within` >>
/// `between` is the measurable claim that a signature is *purer* than the
/// alternative; a signature that merely fragments the class moves `within` up
/// without moving `between`, which the two numbers together reveal.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct CohortStats {
    pub spans: usize,
    pub cohorts: usize,
    pub singleton_cohorts: usize,
    pub singleton_spans: usize,
    pub max_cohort: usize,
    /// Spans in cohorts that reach the minimum (may pay a program).
    pub shared_spans: usize,
    /// Spans in cohorts below the minimum (charged directly, no program).
    pub unshared_spans: usize,
    /// Mean pairwise similarity of members *within* a cohort, in permille.
    pub within_milli: u64,
    /// Mean pairwise similarity of cohort representatives *between* cohorts.
    pub between_milli: u64,
    pub within_pairs: u64,
    pub between_pairs: u64,
}

impl CohortStats {
    /// `within - between` in permille: the separation a signature buys. Positive
    /// means a signature's cohorts are tighter than random cross-cohort pairing.
    #[allow(dead_code)]
    pub fn separation_milli(&self) -> i64 {
        self.within_milli as i64 - self.between_milli as i64
    }
}

/// Compute [`CohortStats`] for an explicit cohort list over `data`/`spans`.
#[allow(dead_code)]
pub fn stats_of(cohorts: &[Cohort], data: &[u8], spans: &[Span]) -> CohortStats {
    let mut st = CohortStats {
        spans: spans.len(),
        cohorts: cohorts.len(),
        ..Default::default()
    };
    for c in cohorts {
        if c.members.len() == 1 {
            st.singleton_cohorts += 1;
            st.singleton_spans += 1;
        }
        st.max_cohort = st.max_cohort.max(c.members.len());
        if c.shared {
            st.shared_spans += c.members.len();
        } else {
            st.unshared_spans += c.members.len();
        }
    }

    // One gram set per span, computed once.
    let gs: Vec<BTreeSet<u64>> = spans
        .iter()
        .map(|s| grams(&data[s.start..s.end()]))
        .collect();

    let mut within_sum = 0u128;
    let mut within_pairs = 0u64;
    for c in cohorts {
        if c.members.len() < 2 {
            continue;
        }
        let stride = stride(c.members.len(), 32);
        let mut i = 0usize;
        while i < c.members.len() {
            let mut j = i + 1;
            while j < c.members.len() {
                within_sum += jaccard_permille(&gs[c.members[i]], &gs[c.members[j]]) as u128;
                within_pairs += 1;
                j += stride;
            }
            i += stride;
        }
    }

    // Representatives: the first (corpus-order) member of each cohort.
    let reps: Vec<usize> = cohorts.iter().map(|c| c.members[0]).collect();
    let mut between_sum = 0u128;
    let mut between_pairs = 0u64;
    let stride = stride(reps.len(), 64);
    let mut i = 0usize;
    while i < reps.len() {
        let mut j = i + 1;
        while j < reps.len() {
            between_sum += jaccard_permille(&gs[reps[i]], &gs[reps[j]]) as u128;
            between_pairs += 1;
            j += stride;
        }
        i += stride;
    }

    st.within_pairs = within_pairs;
    st.between_pairs = between_pairs;
    st.within_milli = if within_pairs == 0 {
        0
    } else {
        (within_sum / within_pairs as u128) as u64
    };
    st.between_milli = if between_pairs == 0 {
        0
    } else {
        (between_sum / between_pairs as u128) as u64
    };
    st
}

/// Cohort statistics for the §14.13 signatures at an explicit minimum size.
#[allow(dead_code)]
pub fn stats(data: &[u8], spans: &[Span], class: ClassName, min_cohort: usize) -> CohortStats {
    stats_of(
        &discover_with_min(data, spans, class, min_cohort),
        data,
        spans,
    )
}

/// Cohort statistics for the pre-§14.13 signatures, for comparison.
#[allow(dead_code)]
pub fn stats_v1(data: &[u8], spans: &[Span], class: ClassName, min_cohort: usize) -> CohortStats {
    stats_of(
        &discover_v1_with_min(data, spans, class, min_cohort),
        data,
        spans,
    )
}

#[cfg(test)]
mod tests {
    use super::super::extract;
    use super::*;

    fn ex(data: &[u8], class: ClassName) -> Vec<Span> {
        extract::extract(data, class, 1000, 1, 1 << 20).spans
    }

    #[test]
    fn template_signature_is_the_sorted_unique_name_set() {
        assert_eq!(first_level_names(b"{{cite|a=1}}"), vec![b"cite".to_vec()]);
        assert_eq!(
            first_level_names(b"{{B|z}}{{a|y}}"),
            vec![b"B".to_vec(), b"a".to_vec()]
        );
        // A nested template is a parameter value, not a second name.
        assert_eq!(
            first_level_names(b"{{outer|p={{inner|x}}}}"),
            vec![b"outer".to_vec()]
        );
    }

    #[test]
    fn template_signature_captures_top_level_parameter_names() {
        assert_eq!(first_level_params(b"{{cite|a=1}}"), vec![b"a".to_vec()]);
        // Positional parameters have no name and are not captured.
        assert!(first_level_params(b"{{cite|1|2}}").is_empty());
        // Nested parameters belong to the nested template.
        assert_eq!(
            first_level_params(b"{{outer|p={{inner|q=1}}}}"),
            vec![b"p".to_vec()]
        );
        // Sorted and unique.
        assert_eq!(
            first_level_params(b"{{c|b=1|a=2|a=3}}"),
            vec![b"a".to_vec(), b"b".to_vec()]
        );
    }

    #[test]
    fn same_signature_set_shares_a_cohort_and_different_sets_do_not() {
        let data = b"{{cite|url=1}} {{cite|url=2}} {{cite|title=3}} {{other|url=4}}";
        let spans = ex(data, ClassName::Template);
        let cohorts = discover(data, &spans, ClassName::Template);
        // `{{cite|url=1}}` and `{{cite|url=2}}`: same names + params -> one cohort.
        let cite_url = cohorts
            .iter()
            .find(|c| c.members.contains(&0) && c.members.contains(&1));
        assert!(cite_url.is_some(), "same (names, params) should share");
        // `{{cite|title=3}}`: same name, different param -> a different cohort.
        let t = cohorts.iter().find(|c| c.members == vec![2]);
        assert!(t.is_some(), "a different param set must split the cohort");
        // `{{other|url=4}}`: different name -> a different cohort.
        let o = cohorts.iter().find(|c| c.members == vec![3]);
        assert!(o.is_some(), "a different name set must split the cohort");
    }

    #[test]
    fn xml_signature_is_the_tag_plus_the_attribute_name_set() {
        assert_eq!(tag_name(b"<page>"), Some(b"page".to_vec()));
        assert_eq!(tag_name(b"<page id=\"1\">"), Some(b"page".to_vec()));
        assert_eq!(tag_name(b"</page>"), None);
        assert_eq!(
            attribute_names(b"<page id=\"1\" class='x'>"),
            vec![b"class".to_vec(), b"id".to_vec()]
        );
        // Values differ, names do not: the same signature.
        assert_eq!(
            signature(ClassName::XmlOpen, b"<page id=\"1\">"),
            signature(ClassName::XmlOpen, b"<page id=\"22\">")
        );
        assert_ne!(
            signature(ClassName::XmlOpen, b"<page id=\"1\">"),
            signature(ClassName::XmlOpen, b"<page name=\"1\">")
        );
    }

    #[test]
    fn link_signature_is_namespace_and_target_shape() {
        // Same namespace and shape, different payload: same signature.
        assert_eq!(
            signature(ClassName::WikiLink, b"[[Category:Alpha]]"),
            signature(ClassName::WikiLink, b"[[Category:Beta]]")
        );
        // A different namespace splits.
        assert_ne!(
            signature(ClassName::WikiLink, b"[[Category:X]]"),
            signature(ClassName::WikiLink, b"[[File:X]]")
        );
        // A piped (display) link is a different shape than a bare one.
        assert_ne!(
            signature(ClassName::WikiLink, b"[[Foo]]"),
            signature(ClassName::WikiLink, b"[[Foo|bar]]")
        );
    }

    #[test]
    fn table_signature_is_column_count_and_type_shape() {
        let one = b"{|\n|1||2||3\n|}";
        let two = b"{|\n|1||2||3\n|-\n|1||2||3\n|}";
        // Same column count and types -> same signature.
        assert_eq!(
            signature(ClassName::WikiTable, one),
            signature(ClassName::WikiTable, two)
        );
        let wide = b"{|\n|1||2||3||4\n|}";
        assert_ne!(
            signature(ClassName::WikiTable, one),
            signature(ClassName::WikiTable, wide)
        );
        let numeric = b"{|\n|1||2||3\n|}";
        let text = b"{|\n|a||b||c\n|}";
        assert_ne!(
            signature(ClassName::WikiTable, numeric),
            signature(ClassName::WikiTable, text)
        );
    }

    #[test]
    fn cohort_discovery_is_stable_and_orders_members_by_position() {
        let data = b"{{a|1}} {{b|2}} {{a|3}}";
        let spans = ex(data, ClassName::Template);
        let c1 = discover(data, &spans, ClassName::Template);
        let c2 = discover(data, &spans, ClassName::Template);
        assert_eq!(c1, c2);
        assert_eq!(c1.len(), 2);
        // The `a` cohort has members at span positions 0 and 2.
        let a = c1.iter().find(|c| c.members.len() == 2).unwrap();
        assert_eq!(a.members, vec![0, 2]);
        assert!(a.members.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn the_minimum_marks_singletons_unshared_and_names_no_program() {
        let data = b"{{a|1}} {{b|2}} {{a|3}}";
        let spans = ex(data, ClassName::Template);
        let cohorts = discover_with_min(data, &spans, ClassName::Template, 3);
        // The `b` singleton cannot share.
        let b = cohorts.iter().find(|c| c.members == vec![1]).unwrap();
        assert!(!b.shared, "a singleton must not be marked shared");
        // The `a` pair is also below 3.
        let a = cohorts.iter().find(|c| c.members.len() == 2).unwrap();
        assert!(!a.shared, "a 2-member cohort is below the default minimum");
        // With min = 2 the pair becomes shareable but the singleton does not.
        let cohorts2 = discover_with_min(data, &spans, ClassName::Template, 2);
        assert!(
            cohorts2
                .iter()
                .find(|c| c.members.len() == 2)
                .unwrap()
                .shared
        );
        assert!(
            !cohorts2
                .iter()
                .find(|c| c.members == vec![1])
                .unwrap()
                .shared
        );
    }

    #[test]
    fn random_control_preserves_the_size_distribution() {
        let data = b"{{a|1}} {{b|2}} {{a|3}} {{c|4}} {{a|5}}";
        let spans = ex(data, ClassName::Template);
        let real = discover(data, &spans, ClassName::Template);
        let mut rng = Rng::new(0x9e37_79b9_7f4a_7c15);
        let rand = randomised(&real, &mut rng);
        let mut real_sizes: Vec<usize> = real.iter().map(|c| c.members.len()).collect();
        let mut rand_sizes: Vec<usize> = rand.iter().map(|c| c.members.len()).collect();
        real_sizes.sort_unstable();
        rand_sizes.sort_unstable();
        assert_eq!(real_sizes, rand_sizes);
        // Every member appears exactly once.
        let mut all: Vec<usize> = rand.iter().flat_map(|c| c.members.clone()).collect();
        all.sort_unstable();
        assert_eq!(all, (0..spans.len()).collect::<Vec<_>>());
        // Deterministic given the seed.
        let mut rng2 = Rng::new(0x9e37_79b9_7f4a_7c15);
        assert_eq!(rand, randomised(&real, &mut rng2));
    }

    #[test]
    fn jaccard_is_bounded_and_orders_identical_members_highest() {
        let a = grams(b"{{cite|url=1}}");
        let b = grams(b"{{cite|url=2}}");
        let c = grams(b"totally unrelated bytes entirely");
        assert_eq!(jaccard_permille(&a, &a), 1000);
        assert!(jaccard_permille(&a, &b) > jaccard_permille(&a, &c));
    }

    /// Measures the §14.13 signatures against the old ones on the development
    /// rung, printing the numbers the report quotes. Deliberate, not part of the
    /// default run:
    ///
    /// ```text
    /// tools/memcap.sh 8 cargo test --release --lib --features procedural,opportunity \
    ///     -- --ignored --nocapture procedure::cohort::tests::enwik6_signature_stats
    /// ```
    #[test]
    #[ignore = "measures enwik6; run deliberately"]
    fn enwik6_signature_stats() {
        let data = std::fs::read("evidence/corpus/enwik6").expect("enwik6");
        for class in ClassName::ALL {
            let spans = ex(&data, class);
            if spans.is_empty() {
                continue;
            }
            let new = stats(&data, &spans, class, DEFAULT_MIN_COHORT);
            let old = stats_v1(&data, &spans, class, DEFAULT_MIN_COHORT);
            eprintln!(
                "{:11} spans={:5} | v1: coh={:5} singles={:4} within={:4} between={:4} sep={:4} | new: coh={:5} singles={:4} within={:4} between={:4} sep={:4} max={:4}",
                class.name(),
                new.spans,
                old.cohorts,
                old.singleton_spans,
                old.within_milli,
                old.between_milli,
                old.separation_milli(),
                new.cohorts,
                new.singleton_spans,
                new.within_milli,
                new.between_milli,
                new.separation_milli(),
                new.max_cohort,
            );
        }
    }
}
