//! ZIR-0: a source-native, exactly-reversible tokenisation of Wikipedia XML.
//!
//! VOLE-Camera's lesson transfers directly to text: do **not** flatten the
//! observation into an undifferentiated byte stream and then try to rediscover
//! structure. Parse source-native structure first, and let typed factorization
//! pay for itself downstream (§8).
//!
//! The hard invariant is exactness. Tokenisation is a *partition* of the input
//! into adjacent byte spans; `render(input, tokenize(input)) == input` for
//! arbitrary input, including malformed and truncated XML. Unknown or malformed
//! constructs fall back to literal spans, so the transform can never lose a
//! byte. The IR is valuable only if the typed factorization reduces total
//! description length downstream — that is measured, never assumed.
//!
//! This is a *research IR*, not a frozen wire format (§12). It exists so that
//! later phases (structural hoisting, typed streams, grammar induction) have a
//! stable, auditable substrate.

use std::ops::Range;

/// A typed span of the input. `start..start+len` indexes the original bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub start: u32,
    pub len: u32,
}

impl Token {
    pub fn range(&self) -> Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

/// The ZIR-0 type system. Every variant is reconstructible as a literal byte
/// span, so classification never affects decodability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    /// XML open tag, e.g. `<page>`.
    XmlOpen = 0,
    /// XML close tag, e.g. `</page>`.
    XmlClose = 1,
    /// XML self-closing tag, e.g. `<tag/>`.
    XmlEmpty = 2,
    /// XML comment `<!-- ... -->`.
    XmlComment = 3,
    /// XML processing instruction / declaration `<? ... ?>`.
    XmlPi = 4,
    /// CDATA section.
    Cdata = 5,
    /// HTML/XML entity, e.g. `&amp;` or `&#160;`.
    Entity = 6,
    /// `{{ ... }}` template or parser function.
    Template = 7,
    /// `[[ ... ]]` wiki link.
    WikiLink = 8,
    /// `{| ... |}` table.
    WikiTable = 9,
    /// A run of ASCII letters.
    Word = 10,
    /// A run of ASCII digits (and embedded separators are split off).
    Number = 11,
    /// A run of spaces/tabs (not newlines).
    Spaces = 12,
    /// A run of newlines.
    Newlines = 13,
    /// A URL-like run.
    Url = 14,
    /// A run of punctuation/other printable bytes.
    Punct = 15,
    /// Any byte that did not match a richer rule (always length >= 1).
    Raw = 16,
}

impl Kind {
    /// All kinds in stable order; useful for building frequency tables.
    pub const ALL: [Kind; 17] = [
        Kind::XmlOpen,
        Kind::XmlClose,
        Kind::XmlEmpty,
        Kind::XmlComment,
        Kind::XmlPi,
        Kind::Cdata,
        Kind::Entity,
        Kind::Template,
        Kind::WikiLink,
        Kind::WikiTable,
        Kind::Word,
        Kind::Number,
        Kind::Spaces,
        Kind::Newlines,
        Kind::Url,
        Kind::Punct,
        Kind::Raw,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::XmlOpen => "xml_open",
            Kind::XmlClose => "xml_close",
            Kind::XmlEmpty => "xml_empty",
            Kind::XmlComment => "xml_comment",
            Kind::XmlPi => "xml_pi",
            Kind::Cdata => "cdata",
            Kind::Entity => "entity",
            Kind::Template => "template",
            Kind::WikiLink => "wiki_link",
            Kind::WikiTable => "wiki_table",
            Kind::Word => "word",
            Kind::Number => "number",
            Kind::Spaces => "spaces",
            Kind::Newlines => "newlines",
            Kind::Url => "url",
            Kind::Punct => "punct",
            Kind::Raw => "raw",
        }
    }

    pub fn from_index(i: usize) -> Option<Kind> {
        Kind::ALL.get(i).copied()
    }
}

#[inline]
fn is_letter(b: u8) -> bool {
    b.is_ascii_alphabetic()
}

#[inline]
fn is_digit(b: u8) -> bool {
    b.is_ascii_digit()
}

#[inline]
fn is_space(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

/// Tokenise `input` into a partition of adjacent typed spans.
///
/// Guarantees:
/// * spans are contiguous and cover `0..input.len()`;
/// * every span has `len >= 1`;
/// * the token kinds are a deterministic function of the bytes.
pub fn tokenize(input: &[u8]) -> Vec<Token> {
    let n = input.len();
    let mut out: Vec<Token> = Vec::with_capacity(n / 4 + 1);
    let mut i = 0usize;
    while i < n {
        let (kind, len) = classify(input, i);
        debug_assert!(len >= 1 && i + len <= n, "classify produced invalid span");
        out.push(Token {
            kind,
            start: i as u32,
            len: len as u32,
        });
        i += len;
    }
    out
}

/// Classify the construct starting at `i`. Always consumes at least one byte.
fn classify(input: &[u8], i: usize) -> (Kind, usize) {
    let n = input.len();
    let b = input[i];

    // --- XML / HTML constructs ------------------------------------------------
    if b == b'<' {
        if let Some(len) = scan_comment(input, i) {
            return (Kind::XmlComment, len);
        }
        if let Some(len) = scan_pi(input, i) {
            return (Kind::XmlPi, len);
        }
        if let Some(len) = scan_cdata(input, i) {
            return (Kind::Cdata, len);
        }
        if let Some(len) = scan_tag(input, i) {
            return (len.2, len.1);
        }
    }

    // --- Entities -------------------------------------------------------------
    if b == b'&' {
        if let Some(len) = scan_entity(input, i) {
            return (Kind::Entity, len);
        }
    }

    // --- Wiki constructs ------------------------------------------------------
    // `{{ ... }}` and `[[ ... ]]`: scan to the balanced closer if one exists on
    // this line region; otherwise fall back to a short literal span.
    if b == b'{' && i + 1 < n && input[i + 1] == b'{' {
        if let Some(len) = scan_balanced(input, i, b'{', b'}') {
            return (Kind::Template, len);
        }
        return (Kind::Punct, 2);
    }
    if b == b'[' && i + 1 < n && input[i + 1] == b'[' {
        if let Some(len) = scan_balanced(input, i, b'[', b']') {
            return (Kind::WikiLink, len);
        }
        return (Kind::Punct, 2);
    }
    if b == b'{' && i + 1 < n && input[i + 1] == b'|' {
        if let Some(len) = scan_table(input, i) {
            return (Kind::WikiTable, len);
        }
        return (Kind::Punct, 2);
    }

    // --- URLs -----------------------------------------------------------------
    if (b == b'h' || b == b'f') && n - i >= 7 {
        if let Some(len) = scan_url(input, i) {
            return (Kind::Url, len);
        }
    }

    // --- Character classes ----------------------------------------------------
    if is_letter(b) {
        let mut j = i + 1;
        while j < n && is_letter(input[j]) {
            j += 1;
        }
        return (Kind::Word, j - i);
    }
    if is_digit(b) {
        let mut j = i + 1;
        while j < n && is_digit(input[j]) {
            j += 1;
        }
        return (Kind::Number, j - i);
    }
    if is_space(b) {
        let mut j = i + 1;
        while j < n && is_space(input[j]) {
            j += 1;
        }
        return (Kind::Spaces, j - i);
    }
    if b == b'\n' || b == b'\r' {
        let mut j = i + 1;
        while j < n && (input[j] == b'\n' || input[j] == b'\r') {
            j += 1;
        }
        return (Kind::Newlines, j - i);
    }
    if b.is_ascii_graphic() {
        // A run of punctuation, stopping at any class boundary.
        let mut j = i + 1;
        while j < n && input[j].is_ascii_graphic() && !is_letter(input[j]) && !is_digit(input[j]) {
            // Do not run into a construct start.
            let c = input[j];
            if c == b'<' || c == b'&' || c == b'[' || c == b'{' {
                break;
            }
            j += 1;
        }
        return (Kind::Punct, j - i);
    }

    // --- Everything else (control bytes, UTF-8 continuation, NUL, ...) --------
    // Consume a short raw run so pathological input cannot fragment into one
    // token per byte, but never cross into an ASCII-class boundary.
    let mut j = i + 1;
    while j < n && !input[j].is_ascii() {
        j += 1;
    }
    (Kind::Raw, j - i)
}

/// Scan `<!-- ... -->`. Returns the span length if a complete comment exists.
fn scan_comment(input: &[u8], i: usize) -> Option<usize> {
    if !input[i..].starts_with(b"<!--") {
        return None;
    }
    let rest = &input[i + 4..];
    rest.windows(3).position(|w| w == b"-->").map(|p| 4 + p + 3)
}

/// Scan `<? ... ?>`.
fn scan_pi(input: &[u8], i: usize) -> Option<usize> {
    if !input[i..].starts_with(b"<?") {
        return None;
    }
    let rest = &input[i + 2..];
    rest.windows(2).position(|w| w == b"?>").map(|p| 2 + p + 2)
}

/// Scan `<![CDATA[ ... ]]>`.
fn scan_cdata(input: &[u8], i: usize) -> Option<usize> {
    if !input[i..].starts_with(b"<![CDATA[") {
        return None;
    }
    let rest = &input[i + 9..];
    rest.windows(3).position(|w| w == b"]]>").map(|p| 9 + p + 3)
}

/// Scan an XML tag. Returns `(is_open, length, kind)`.
fn scan_tag(input: &[u8], i: usize) -> Option<(bool, usize, Kind)> {
    let n = input.len();
    debug_assert_eq!(input[i], b'<');
    if i + 1 >= n {
        return None;
    }
    let next = input[i + 1];
    // A tag name must start with a letter, ':', or '_'.
    let is_close = next == b'/';
    let ns = if is_close { i + 2 } else { i + 1 };
    if ns >= n {
        return None;
    }
    let c = input[ns];
    if !(c.is_ascii_alphabetic() || c == b':' || c == b'_') {
        return None;
    }
    // Find the closing '>'.
    let mut j = ns;
    while j < n {
        match input[j] {
            b'>' => {
                let len = j - i + 1;
                let self_closing = j > i && input[j - 1] == b'/';
                let kind = if is_close {
                    Kind::XmlClose
                } else if self_closing {
                    Kind::XmlEmpty
                } else {
                    Kind::XmlOpen
                };
                return Some((!is_close && !self_closing, len, kind));
            }
            // Do not let a tag run across a newline or absorb a nested '<'.
            b'\n' | b'<' => return None,
            _ => j += 1,
        }
    }
    None
}

/// Scan `&...;` with a plausible length.
fn scan_entity(input: &[u8], i: usize) -> Option<usize> {
    let n = input.len();
    let mut j = i + 1;
    // `&#123;` or `&#x1F;` or `&name;`
    if j < n && input[j] == b'#' {
        j += 1;
        if j < n && (input[j] == b'x' || input[j] == b'X') {
            j += 1;
        }
    }
    let digits_start = j;
    while j < n && (input[j].is_ascii_alphanumeric() || input[j] == b'#') && j - i < 34 {
        j += 1;
    }
    if j == digits_start || j >= n || input[j] != b';' {
        return None;
    }
    Some(j - i + 1)
}

/// Scan a balanced `open ... close` construct with a depth counter, bounded to
/// avoid pathological scanning on malformed input.
fn scan_balanced(input: &[u8], i: usize, open: u8, close: u8) -> Option<usize> {
    const MAX_SCAN: usize = 1 << 16;
    let n = input.len();
    let mut depth = 0i32;
    let mut j = i;
    let end = n.min(i + MAX_SCAN);
    while j < end {
        let b = input[j];
        if b == open {
            depth += 1;
            j += 1;
        } else if b == close {
            depth -= 1;
            j += 1;
            if depth == 0 {
                return Some(j - i);
            }
        } else {
            j += 1;
        }
    }
    None
}

/// Scan a `{| ... |}` table.
fn scan_table(input: &[u8], i: usize) -> Option<usize> {
    const MAX_SCAN: usize = 1 << 20;
    let n = input.len();
    let end = n.min(i + MAX_SCAN);
    let mut j = i + 2;
    while j + 1 < end {
        if input[j] == b'|' && input[j + 1] == b'}' {
            return Some(j + 2 - i);
        }
        j += 1;
    }
    None
}

/// Scan `http://`, `https://`, `ftp://`, `//` prefixes, stopping at whitespace
/// or a delimiter that ends the URL.
fn scan_url(input: &[u8], i: usize) -> Option<usize> {
    let schemes: [&[u8]; 4] = [b"http://", b"https://", b"ftp://", b"ftps://"];
    let mut matched = None;
    for s in schemes {
        if input[i..].starts_with(s) {
            matched = Some(s.len());
            break;
        }
    }
    let start = matched?;
    let n = input.len();
    let mut j = i + start;
    while j < n {
        let b = input[j];
        if b.is_ascii_whitespace() || b == b'<' || b == b'>' || b == b'"' || b == b']' || b == b'|'
        {
            break;
        }
        j += 1;
    }
    Some(j - i)
}

/// Reconstruct the exact original bytes from a token list.
///
/// Panics (in debug) if the tokens are not a contiguous partition, because that
/// would indicate a tokeniser bug rather than a property of the input.
pub fn render(input: &[u8], tokens: &[Token]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut expected = 0usize;
    for t in tokens {
        debug_assert_eq!(t.start as usize, expected, "tokens are not contiguous");
        let r = t.range();
        debug_assert!(r.end <= input.len(), "token out of bounds");
        expected = r.end;
        out.extend_from_slice(&input[r]);
    }
    debug_assert_eq!(expected, input.len(), "tokens do not cover the input");
    out
}

/// Per-kind byte and token statistics, used to decide whether a split pays.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub token_count: [u64; 17],
    pub byte_count: [u64; 17],
}

impl Stats {
    pub fn of(input: &[u8], tokens: &[Token]) -> Stats {
        let mut s = Stats::default();
        for t in tokens {
            s.token_count[t.kind as usize] += 1;
            s.byte_count[t.kind as usize] += t.len as u64;
        }
        let _ = input;
        s
    }

    pub fn total_bytes(&self) -> u64 {
        self.byte_count.iter().sum()
    }

    pub fn render_table(&self) -> String {
        let mut s = String::new();
        s.push_str("| kind | tokens | bytes |\n|---|---|---|\n");
        for k in Kind::ALL {
            let i = k as usize;
            s.push_str(&format!(
                "| {} | {} | {} |\n",
                k.name(),
                self.token_count[i],
                self.byte_count[i]
            ));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(data: &[u8]) {
        let toks = tokenize(data);
        let out = render(data, &toks);
        assert_eq!(out, data, "roundtrip failed");
        // Contiguity and coverage.
        let mut expected = 0usize;
        for t in &toks {
            assert_eq!(t.start as usize, expected);
            assert!(t.len >= 1);
            expected = t.range().end;
        }
        assert_eq!(expected, data.len());
    }

    #[test]
    fn roundtrip_simple() {
        roundtrip(b"<page><title>Hello</title>\n{{cite|a=1}}\n[[Link]] text 123 &amp;\n</page>");
    }

    #[test]
    fn roundtrip_empty() {
        roundtrip(b"");
    }

    #[test]
    fn roundtrip_binary() {
        let data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        roundtrip(&data);
    }

    #[test]
    fn roundtrip_malformed() {
        for s in [
            &b"<page"[..],
            &b"{{unterminated"[..],
            &b"[[also unterminated"[..],
            &b"<![CDATA[ unfinished"[..],
            &b"& never ends"[..],
            &b"<a><b></a>"[..],
            &b"\xff\xfe\x00\x01garbage"[..],
            &b"{| table"[..],
        ] {
            roundtrip(s);
        }
    }

    #[test]
    fn kinds_are_reasonable() {
        let data = b"<title>Foo</title> &amp; 123 [[Bar]]";
        let toks = tokenize(data);
        let kinds: Vec<Kind> = toks.iter().map(|t| t.kind).collect();
        assert!(kinds.contains(&Kind::XmlOpen), "{kinds:?}");
        assert!(kinds.contains(&Kind::XmlClose));
        assert!(kinds.contains(&Kind::Word));
        assert!(kinds.contains(&Kind::Number));
        assert!(kinds.contains(&Kind::Entity));
        assert!(kinds.contains(&Kind::WikiLink));
    }

    #[test]
    fn property_random_roundtrip() {
        // Deterministic pseudo-random byte soup with injected constructs.
        let mut s = 0x1234_5678_9abc_def0u64;
        let alphabet = b"<>[]{}|&=;/abcXYZ019 \n\t\xff\xc3\xa9";
        for _ in 0..200 {
            let mut data = Vec::new();
            for _ in 0..300 {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                data.push(alphabet[(s as usize) % alphabet.len()]);
            }
            roundtrip(&data);
        }
    }

    #[test]
    fn stats_cover_input() {
        let data = b"<a>x</a>";
        let toks = tokenize(data);
        let st = Stats::of(data, &toks);
        assert_eq!(st.total_bytes(), data.len() as u64);
    }
}
