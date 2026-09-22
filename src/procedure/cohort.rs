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
//! Three signatures are implemented, exactly as the plan names them:
//!
//! * `template` — the sorted unique set of **first-level template names** in the
//!   span (the name before the first `|`, at brace depth 0);
//! * `xml_open` — the **tag name**;
//! * everything else — the **shape**: the sequence of IR `Kind`s the span
//!   tokenises to.
//!
//! The signature bytes are hashed (FNV-1a, our own, so it cannot drift with a
//! standard-library change) to a 64-bit cohort id. Cohorts are ordered by
//! signature bytes, members by corpus position, so no hashing order leaks into
//! the output.
//!
//! `randomised` is the §14.44 negative control: it keeps the exact cohort *size
//! distribution* but assigns members to cohorts uniformly at random from a
//! seeded, deterministic PRNG. A real cohorting gain must beat it.

use std::collections::BTreeMap;

use crate::ir;

use super::extract::{ClassName, Span};

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

/// The cohort signature of a span for a given class.
pub fn signature(class: ClassName, span: &[u8]) -> Vec<u8> {
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

/// A discovered cohort: an id (the signature hash) and member indices into the
/// span list, in corpus order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cohort {
    pub id: u64,
    pub members: Vec<usize>,
}

/// Group spans into cohorts by signature. Cohorts are ordered by signature
/// bytes; members are ordered by corpus position. Deterministic by construction.
pub fn discover(data: &[u8], spans: &[Span], class: ClassName) -> Vec<Cohort> {
    let mut groups: BTreeMap<Vec<u8>, Vec<usize>> = BTreeMap::new();
    for (i, s) in spans.iter().enumerate() {
        let sig = signature(class, &data[s.start..s.end()]);
        groups.entry(sig).or_default().push(i);
    }
    groups
        .into_iter()
        .map(|(sig, mut members)| {
            members.sort_unstable();
            Cohort {
                id: fnv1a(&sig),
                members,
            }
        })
        .collect()
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
            members,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::extract;
    use super::*;

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
        // Sorting is by bytes, so `B` < `a`.
        assert_eq!(
            signature(ClassName::Template, b"{{B|z}}{{a|y}}"),
            b"B\x1fa".to_vec()
        );
    }

    #[test]
    fn xml_signature_is_the_tag_name() {
        assert_eq!(tag_name(b"<page>"), Some(b"page".to_vec()));
        assert_eq!(tag_name(b"<page id=\"1\">"), Some(b"page".to_vec()));
        assert_eq!(tag_name(b"</page>"), None);
        assert_eq!(signature(ClassName::XmlOpen, b"<page>"), b"page".to_vec());
    }

    #[test]
    fn cohort_discovery_is_stable_and_orders_members_by_position() {
        let data = b"{{a|1}} {{b|2}} {{a|3}}";
        let ex = extract::extract(data, ClassName::Template, 100, 1, 1 << 20);
        let c1 = discover(data, &ex.spans, ClassName::Template);
        let c2 = discover(data, &ex.spans, ClassName::Template);
        assert_eq!(c1, c2);
        assert_eq!(c1.len(), 2);
        // The `a` cohort has members at span positions 0 and 2.
        let a = c1.iter().find(|c| c.members.len() == 2).unwrap();
        assert_eq!(a.members, vec![0, 2]);
        assert!(a.members.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn random_control_preserves_the_size_distribution() {
        let data = b"{{a|1}} {{b|2}} {{a|3}} {{c|4}} {{a|5}}";
        let ex = extract::extract(data, ClassName::Template, 100, 1, 1 << 20);
        let real = discover(data, &ex.spans, ClassName::Template);
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
        assert_eq!(all, (0..ex.spans.len()).collect::<Vec<_>>());
        // Deterministic given the seed.
        let mut rng2 = Rng::new(0x9e37_79b9_7f4a_7c15);
        assert_eq!(rand, randomised(&real, &mut rng2));
    }
}
