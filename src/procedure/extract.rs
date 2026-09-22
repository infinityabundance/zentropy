//! Class extraction: pull the *targets* of one structural class out of a corpus.
//!
//! A target is a maximal IR span of the requested [`Kind`]. `extract` is a pure
//! function of `(bytes, class, limit, min_span, max_span)`:
//!
//! * it tokenises the whole corpus with [`crate::ir::tokenize`] (the same
//!   partition the rest of the pipeline trusts, so `render(tokenize(x)) == x`);
//! * it keeps only the spans whose `Kind` is the class's kind;
//! * it **drops** — never repairs — any span that is out of bounds, empty, or
//!   overlaps a previously accepted span, and any span outside the size window;
//! * it stops at `limit` accepted spans.
//!
//! The drop count is returned rather than hidden: an experiment whose inputs were
//! silently synthesised is not a measurement of anything.

use crate::ir::{self, Kind};

/// The class names the experiment accepts, exactly the IR [`Kind`] names that
/// name a structural construct (plus `all`, handled by the caller as "run each").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassName {
    Template,
    WikiLink,
    WikiTable,
    XmlOpen,
    Number,
    Url,
    Entity,
}

impl ClassName {
    /// Every named class, in the report's canonical order.
    pub const ALL: [ClassName; 7] = [
        ClassName::Template,
        ClassName::WikiLink,
        ClassName::WikiTable,
        ClassName::XmlOpen,
        ClassName::Number,
        ClassName::Url,
        ClassName::Entity,
    ];

    /// The IR kind this class selects.
    pub fn kind(self) -> Kind {
        match self {
            ClassName::Template => Kind::Template,
            ClassName::WikiLink => Kind::WikiLink,
            ClassName::WikiTable => Kind::WikiTable,
            ClassName::XmlOpen => Kind::XmlOpen,
            ClassName::Number => Kind::Number,
            ClassName::Url => Kind::Url,
            ClassName::Entity => Kind::Entity,
        }
    }

    /// The class's command-line name (identical to the IR kind's name).
    pub fn name(self) -> &'static str {
        self.kind().name()
    }

    /// Parse a `--class` value. `all` is deliberately *not* a `ClassName`: it is
    /// a selector (run each class), not a class.
    pub fn from_name(s: &str) -> Option<ClassName> {
        ClassName::ALL.into_iter().find(|c| c.name() == s)
    }
}

/// A selected target: a contiguous byte range of the corpus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub len: usize,
}

impl Span {
    pub fn end(self) -> usize {
        self.start + self.len
    }
}

/// The outcome of extraction: the accepted spans and how many candidates were
/// dropped (out of bounds / empty / overlapping / outside the size window).
#[derive(Clone, Debug, Default)]
pub struct Extracted {
    pub spans: Vec<Span>,
    pub dropped: usize,
}

/// Extract the accepted targets of `class` from `data`.
pub fn extract(
    data: &[u8],
    class: ClassName,
    limit: usize,
    min_span: usize,
    max_span: usize,
) -> Extracted {
    let kind = class.kind();
    let mut out = Extracted::default();
    if limit == 0 {
        return out;
    }
    let tokens = ir::tokenize(data);
    let mut prev_end = 0usize;
    for t in &tokens {
        if t.kind != kind {
            continue;
        }
        let start = t.start as usize;
        let len = t.len as usize;
        let end = start + len;
        // Malformed: the token does not name a non-empty in-bounds range. Drop
        // it; a repaired span would be a different target than the corpus has.
        if len == 0 || end > data.len() {
            out.dropped += 1;
            continue;
        }
        // Overlap: `tokenize` is contiguous so this is defensive, but the rule is
        // "drop, never fudge" and it is cheap to enforce.
        if start < prev_end {
            out.dropped += 1;
            continue;
        }
        if len < min_span || len > max_span {
            out.dropped += 1;
            continue;
        }
        if out.spans.len() >= limit {
            break;
        }
        out.spans.push(Span { start, len });
        prev_end = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_the_requested_kind() {
        let data = b"<page>{{cite|x=1}} [[Link]] 123 &amp;</page>";
        let t = extract(data, ClassName::Template, 100, 1, 1 << 20);
        assert_eq!(t.spans.len(), 1);
        assert_eq!(&data[t.spans[0].start..t.spans[0].end()], b"{{cite|x=1}}");

        let l = extract(data, ClassName::WikiLink, 100, 1, 1 << 20);
        assert_eq!(&data[l.spans[0].start..l.spans[0].end()], b"[[Link]]");

        let n = extract(data, ClassName::Number, 100, 1, 1 << 20);
        assert_eq!(&data[n.spans[0].start..n.spans[0].end()], b"123");

        let e = extract(data, ClassName::Entity, 100, 1, 1 << 20);
        assert_eq!(&data[e.spans[0].start..e.spans[0].end()], b"&amp;");
    }

    #[test]
    fn limit_bounds_the_target_count() {
        let data = b"<a></a><b></b><c></c><d></d>";
        let t = extract(data, ClassName::XmlOpen, 2, 1, 1 << 20);
        assert_eq!(t.spans.len(), 2);
        assert_eq!(&data[t.spans[0].start..t.spans[0].end()], b"<a>");
        assert_eq!(&data[t.spans[1].start..t.spans[1].end()], b"<b>");
    }

    #[test]
    fn size_window_drops_rather_than_clips() {
        let data = b"{{a}}{{longer|parameter=value}}";
        // min 10 drops the 5-byte `{{a}}`, leaving the long one.
        let t = extract(data, ClassName::Template, 100, 10, 1 << 20);
        assert_eq!(t.spans.len(), 1);
        assert!(t.dropped >= 1);
        assert!(t.spans[0].len >= 10);
    }

    #[test]
    fn extraction_is_deterministic_and_spans_do_not_overlap() {
        let data = b"{{a|x}}{{b|y}}{{c|z}}";
        let a = extract(data, ClassName::Template, 100, 1, 1 << 20);
        let b = extract(data, ClassName::Template, 100, 1, 1 << 20);
        assert_eq!(a.spans, b.spans);
        for w in a.spans.windows(2) {
            assert!(w[0].end() <= w[1].start, "spans overlap: {w:?}");
        }
    }

    #[test]
    fn all_named_classes_resolve_to_distinct_kinds() {
        let mut kinds: Vec<Kind> = ClassName::ALL.iter().map(|c| c.kind()).collect();
        kinds.sort_by_key(|k| *k as u8);
        kinds.dedup();
        assert_eq!(kinds.len(), ClassName::ALL.len());
    }
}
