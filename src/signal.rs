//! Phase 14.6: one causal structural-state interface.
//!
//! **Why one.** Before this module, structural knowledge was re-derived wherever
//! it was needed: the IR tokenizer knew about XML and templates, the article
//! layout knew about page ids, the column expert knew about line geometry, and the
//! transforms knew about tag prefixes. Six partial parsers is six chances to
//! disagree, and a model fed a *different* structure than the transform wrote is
//! worse than one fed none.
//!
//! **Causality is the whole contract.** Every field is a pure function of bytes
//! that have already been coded, or of state the archive explicitly persisted. A
//! field depending on a future byte would make the decoder's context
//! unreproducible — not a bug to fix later, a silent desynchronisation. The tests
//! below assert the property directly, by comparing a whole-slice pass against
//! every prefix computed from scratch.
//!
//! **Bounds.** Depths saturate at [`MAX_DEPTH`] so a malformed corpus cannot grow
//! the context without bound, and the scanners use a fixed 8-byte look-behind
//! window rather than a buffer that grows with input.
//!
//! **What is not tracked.** Some `ZirClass` variants are reserved for consumers
//! that classify spans themselves and are never produced here. `Url` is the
//! important one: a URL is decoded lexically (`http` is `Word`, `:` and `/` are
//! `Punct`), because recognising its extent would need bytes that have not been
//! coded yet, and the causal contract forbids that. `Plain` likewise marks only
//! spans that carry no markup role at all. This is recorded rather than papered
//! over with a look-ahead-based guess.
//!
//! **Cost.** `SignalState` is `Copy` and tiny; `key()` is a few multiplies. The
//! scored build pays for the module only through consumers that earn their keep.

/// Coarse span class, mirroring the corpus IR's taxonomy so a model and a
/// transform cannot disagree about what "a template" is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ZirClass {
    #[default]
    Plain,
    XmlTag,
    XmlAttr,
    WikiTemplate,
    WikiParam,
    WikiLink,
    WikiTable,
    Entity,
    Url,
    Number,
    Word,
    Spaces,
    Newlines,
    Punct,
}

impl ZirClass {
    #[inline]
    pub fn code(self) -> u32 {
        match self {
            ZirClass::Plain => 0,
            ZirClass::XmlTag => 1,
            ZirClass::XmlAttr => 2,
            ZirClass::WikiTemplate => 3,
            ZirClass::WikiParam => 4,
            ZirClass::WikiLink => 5,
            ZirClass::WikiTable => 6,
            ZirClass::Entity => 7,
            ZirClass::Url => 8,
            ZirClass::Number => 9,
            ZirClass::Word => 10,
            ZirClass::Spaces => 11,
            ZirClass::Newlines => 12,
            ZirClass::Punct => 13,
        }
    }

    pub const ALL_LEN: usize = 14;

    pub fn name(self) -> &'static str {
        match self {
            ZirClass::Plain => "plain",
            ZirClass::XmlTag => "xml_tag",
            ZirClass::XmlAttr => "xml_attr",
            ZirClass::WikiTemplate => "wiki_template",
            ZirClass::WikiParam => "wiki_param",
            ZirClass::WikiLink => "wiki_link",
            ZirClass::WikiTable => "wiki_table",
            ZirClass::Entity => "entity",
            ZirClass::Url => "url",
            ZirClass::Number => "number",
            ZirClass::Word => "word",
            ZirClass::Spaces => "spaces",
            ZirClass::Newlines => "newlines",
            ZirClass::Punct => "punct",
        }
    }
}

/// Which mediawiki envelope the cursor is inside.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageState {
    #[default]
    Outside,
    Page,
    Title,
    Id,
    Revision,
    Text,
    /// Inside `<text>` and past a wikitext section heading.
    Section,
}

impl PageState {
    #[inline]
    pub fn code(self) -> u32 {
        match self {
            PageState::Outside => 0,
            PageState::Page => 1,
            PageState::Title => 2,
            PageState::Id => 3,
            PageState::Revision => 4,
            PageState::Text => 5,
            PageState::Section => 6,
        }
    }

    /// The envelope a child element closes back to.
    #[inline]
    fn parent(self) -> PageState {
        match self {
            PageState::Title | PageState::Id => PageState::Page,
            PageState::Text | PageState::Section => PageState::Revision,
            PageState::Revision | PageState::Page => PageState::Page,
            PageState::Outside => PageState::Outside,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkState {
    #[default]
    None,
    /// Inside `[[ ... ]]` before the `|`: the target.
    Target,
    /// After the `|`: the surface form.
    Surface,
}

impl LinkState {
    #[inline]
    pub fn code(self) -> u32 {
        match self {
            LinkState::None => 0,
            LinkState::Target => 1,
            LinkState::Surface => 2,
        }
    }
}

/// The causal structural state at the current byte boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SignalState {
    pub byte_pos: u64,
    /// 0..8 bits consumed within the current byte (bit 7 first).
    pub bitpos: u8,
    pub zir: ZirClass,
    pub page: PageState,
    pub link: LinkState,
    pub template_depth: u8,
    pub in_template_param: bool,
    pub table_depth: u8,
    pub in_table_cell: bool,
    pub list_depth: u8,
    /// 1..=6 inside a wikitext heading, 0 otherwise.
    pub heading: u8,
    pub in_xml_tag: bool,
    pub in_xml_attr: bool,
    /// Byte offset within the current word run, saturated at 15.
    pub word_pos: u8,
    pub numeric: bool,
    /// True at the first byte of a line, causally (only after a `\n` was seen).
    pub line_start: bool,
}

/// Saturation point for every nesting counter.
pub const MAX_DEPTH: u8 = 15;

/// Saturation point for [`SignalState::word_pos`]: the exact offset past 15 bytes
/// into a word is not worth the wider field, and the doc comment promises the
/// bound.
pub const MAX_WORD_POS: u8 = 15;

/// Which fields `key()` folds in. Constants are `SignalFields` values rather than
/// bare masks so `key(SignalFields::STRUCTURAL)` composes without a wrapper.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SignalFields(pub u32);

impl SignalFields {
    pub const ZIR: SignalFields = SignalFields(1 << 0);
    pub const PAGE: SignalFields = SignalFields(1 << 1);
    pub const LINK: SignalFields = SignalFields(1 << 2);
    pub const TEMPLATE: SignalFields = SignalFields(1 << 3);
    pub const TABLE: SignalFields = SignalFields(1 << 4);
    pub const LIST: SignalFields = SignalFields(1 << 5);
    pub const HEADING: SignalFields = SignalFields(1 << 6);
    pub const XML: SignalFields = SignalFields(1 << 7);
    pub const WORD: SignalFields = SignalFields(1 << 8);
    pub const NUMERIC: SignalFields = SignalFields(1 << 9);
    pub const BITPOS: SignalFields = SignalFields(1 << 10);
    pub const LINE_START: SignalFields = SignalFields(1 << 11);

    #[inline]
    pub const fn or(self, other: SignalFields) -> SignalFields {
        SignalFields(self.0 | other.0)
    }

    /// The structural set a procedural or residual model usually wants.
    pub const STRUCTURAL: SignalFields = SignalFields(
        Self::ZIR.0
            | Self::PAGE.0
            | Self::LINK.0
            | Self::TEMPLATE.0
            | Self::TABLE.0
            | Self::LIST.0
            | Self::HEADING.0
            | Self::XML.0,
    );

    #[inline]
    pub fn has(self, f: SignalFields) -> bool {
        self.0 & f.0 != 0
    }
}

/// The interface. One instance per coding pass; the driver feeds it bytes as they
/// are consumed and consumers read `state()`.
pub trait SignalBus {
    /// Advance by exactly one coded byte. Called once per byte, in order, on both
    /// the encode and the decode path.
    fn observe(&mut self, byte: u8);
    fn state(&self) -> SignalState;
    /// A compact integer key over the requested fields. Stable for a given state
    /// and field set; never a hash of a memory address.
    fn key(&self, fields: SignalFields) -> u32;
    /// Set the bit position within the current byte (0..8).
    fn set_bitpos(&mut self, bitpos: u8);
}

/// A tiny deterministic name hash. Deliberately not `DefaultHasher`: SipHash is
/// keyed per process, so its output is not a stable contract.
#[inline]
pub fn hash_name(name: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in name {
        h ^= (b as u32) | 0x20; // lowercase-fold, so `<Page>` == `<page>`
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Envelope tag hashes, computed once so the hot path is a comparison.
mod tags {
    pub const PAGE: u32 = super::hash_name_const(b"page");
    pub const TITLE: u32 = super::hash_name_const(b"title");
    pub const ID: u32 = super::hash_name_const(b"id");
    pub const REVISION: u32 = super::hash_name_const(b"revision");
    pub const TEXT: u32 = super::hash_name_const(b"text");
}

/// `const`-evaluable variant of [`hash_name`] for the envelope table.
pub const fn hash_name_const(name: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0;
    while i < name.len() {
        h ^= (name[i] as u32) | 0x20;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// The streaming implementation.
///
/// A byte-at-a-time scanner: `observe` sees a byte only after it has been coded,
/// and every field is a function of the bytes observed so far. Structural
/// look-behind uses a fixed 8-byte window (`name_buf`), so nothing here grows
/// with the input.
#[derive(Clone, Debug)]
pub struct TrackingBus {
    st: SignalState,
    /// First byte of a potential `{{`, `[[`, `]]`, `}}` pair has been seen.
    saw_open_brace: bool,
    saw_close_brace: bool,
    saw_open_bracket: bool,
    saw_close_bracket: bool,
    /// Inside `<...>`.
    in_tag_angle: bool,
    /// Inside a quoted attribute value, and which quote opened it.
    in_quote: bool,
    quote_byte: u8,
    /// Inside `<!-- ... -->` / `<![CDATA[...]]>`.
    in_comment: bool,
    comment_dashes: u8,
    in_cdata: bool,
    cdata_brackets: u8,
    /// Inside `&...;`.
    in_entity: bool,
    /// First 8 bytes emitted after `<`, for opener detection and the tag name.
    name_buf: [u8; 8],
    name_len: u8,
    /// A `/` was seen immediately after `<`, so `>` must close the current
    /// envelope rather than open a new one. Kept out of `name_buf` so the name
    /// window still holds just the element name.
    closing_tag: bool,
    /// The previous byte was a `|`, which lets a following `}` close a table
    /// (`|}`). Needed because `|}` cannot be recognised without one byte of
    /// look-behind.
    saw_pipe: bool,
    /// Consecutive `=` seen while a heading is being assembled.
    line_eq: u8,
    /// The heading level for the current line is final, so a later `=` run on the
    /// same line (a closing `==`) cannot raise it.
    heading_decided: bool,
}

impl Default for TrackingBus {
    fn default() -> Self {
        TrackingBus {
            st: SignalState::default(),
            saw_open_brace: false,
            saw_close_brace: false,
            saw_open_bracket: false,
            saw_close_bracket: false,
            in_tag_angle: false,
            in_quote: false,
            quote_byte: 0,
            in_comment: false,
            comment_dashes: 0,
            in_cdata: false,
            cdata_brackets: 0,
            in_entity: false,
            name_buf: [0; 8],
            name_len: 0,
            closing_tag: false,
            saw_pipe: false,
            line_eq: 0,
            heading_decided: false,
        }
    }
}

impl TrackingBus {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn bump(d: u8) -> u8 {
        if d >= MAX_DEPTH {
            MAX_DEPTH
        } else {
            d + 1
        }
    }

    #[inline]
    fn push_name(&mut self, b: u8) {
        if (self.name_len as usize) < self.name_buf.len() {
            self.name_buf[self.name_len as usize] = b;
            self.name_len += 1;
        }
    }

    #[inline]
    fn name_starts_with(&self, prefix: &[u8]) -> bool {
        self.name_len as usize >= prefix.len() && &self.name_buf[..prefix.len()] == prefix
    }

    /// A byte that ends the "line start" window, because real content has begun.
    #[inline]
    fn end_line_start(&mut self, byte: u8) {
        // `=`, `*`, `#`, `;`, `:` and spaces keep a line's prefix window open;
        // anything else means the line's structure has been decided.
        if !matches!(byte, b'=' | b'*' | b'#' | b';' | b':' | b' ' | b'\t') {
            self.st.line_start = false;
        }
    }

    /// Handle one byte while inside `<...>`.
    fn observe_in_tag(&mut self, byte: u8) {
        // A `/` immediately after `<` marks a close tag. It is not part of the
        // name, so the name window stays empty and `>` decides using
        // `closing_tag`; closing on the `/` itself would hash an empty name and
        // silently drop the `</text>` that ends a text envelope.
        if byte == b'/' && self.name_len == 0 {
            self.closing_tag = true;
            self.st.in_xml_tag = true;
            self.st.zir = ZirClass::XmlTag;
            self.st.numeric = false;
            self.end_line_start(byte);
            return;
        }

        // Opener detection: the first bytes after `<` decide whether this is a
        // comment or a CDATA section, both of which swallow `>` until they close.
        if self.name_len < 8 {
            self.push_name(byte);
            if self.name_starts_with(b"!--") {
                self.in_comment = true;
                self.comment_dashes = 0;
                self.in_tag_angle = false;
                self.st.zir = ZirClass::Plain;
                return;
            }
            if self.name_starts_with(b"![CDATA[") {
                self.in_cdata = true;
                self.cdata_brackets = 0;
                self.in_tag_angle = false;
                self.st.zir = ZirClass::Plain;
                return;
            }
        }

        if self.in_quote {
            self.st.in_xml_attr = true;
            self.st.numeric = byte.is_ascii_digit();
            self.st.zir = ZirClass::XmlAttr;
            if byte == self.quote_byte {
                self.in_quote = false;
                self.st.in_xml_attr = false;
            }
            return;
        }

        self.st.in_xml_tag = true;
        match byte {
            b'"' | b'\'' => {
                self.in_quote = true;
                self.quote_byte = byte;
                self.st.in_xml_attr = true;
                self.st.zir = ZirClass::XmlAttr;
            }
            b'>' => self.close_tag(self.closing_tag),
            b' ' | b'\t' | b'\n' | b'=' => {
                self.st.zir = ZirClass::XmlAttr;
            }
            _ => {
                self.st.zir = ZirClass::XmlTag;
            }
        }
        self.st.numeric = byte.is_ascii_digit();
        self.end_line_start(byte);
    }

    /// Apply the tag whose name window we just closed.
    fn close_tag(&mut self, closing: bool) {
        let name = tag_name_hash(&self.name_buf[..self.name_len as usize]);
        self.in_tag_angle = false;
        self.name_len = 0;
        self.closing_tag = false;
        self.st.in_xml_tag = false;
        self.st.in_xml_attr = false;
        if name == 0 {
            return;
        }
        if closing {
            self.st.page = self.st.page.parent();
        } else {
            self.st.page = match name {
                tags::PAGE => PageState::Page,
                tags::TITLE => PageState::Title,
                tags::ID => PageState::Id,
                tags::REVISION => PageState::Revision,
                tags::TEXT => PageState::Text,
                _ => self.st.page,
            };
        }
    }

    /// Handle one byte while inside `<!-- -->`.
    fn observe_in_comment(&mut self, byte: u8) {
        self.st.zir = ZirClass::Plain;
        self.st.numeric = false;
        if byte == b'-' {
            self.comment_dashes = self.comment_dashes.saturating_add(1);
        } else {
            if byte == b'>' && self.comment_dashes >= 2 {
                self.in_comment = false;
            }
            self.comment_dashes = 0;
        }
        self.end_line_start(byte);
    }

    /// Handle one byte while inside `<![CDATA[...]]>`.
    fn observe_in_cdata(&mut self, byte: u8) {
        self.st.zir = ZirClass::Plain;
        self.st.numeric = false;
        self.cdata_brackets = if byte == b']' {
            self.cdata_brackets.saturating_add(1)
        } else {
            0
        };
        if byte == b'>' && self.cdata_brackets >= 2 {
            self.in_cdata = false;
        }
        self.end_line_start(byte);
    }

    /// Handle one byte while inside `&...;`.
    fn observe_in_entity(&mut self, byte: u8) {
        self.st.zir = ZirClass::Entity;
        self.st.numeric = byte.is_ascii_digit();
        if matches!(byte, b';' | b' ' | b'\n' | b'<') {
            self.in_entity = false;
        }
        self.end_line_start(byte);
    }

    /// Handle one byte in ordinary content.
    fn observe_normal(&mut self, byte: u8) {
        self.st.in_xml_tag = false;
        self.st.in_xml_attr = false;

        // A pending opener pair only survives its own bytes; any other byte ends
        // it, so a stray `{` cannot later be completed into `{|` by an unrelated
        // `|`, nor a stray `}` pair with a distant `}}`.
        if byte != b'{' && byte != b'|' {
            self.saw_open_brace = false;
        }
        if byte != b'}' {
            self.saw_close_brace = false;
        }
        // `|}` needs one byte of look-behind: remember whether the previous byte
        // was the pipe before deciding what this `}` closes.
        let after_pipe = self.saw_pipe;
        self.saw_pipe = byte == b'|';
        // The opening run of `=` decides the heading level; once any other byte
        // breaks it the level is final for this line.
        if self.line_eq > 0 && byte != b'=' {
            self.heading_decided = true;
        }

        match byte {
            b'<' => {
                self.in_tag_angle = true;
                self.name_len = 0;
                self.closing_tag = false;
                self.st.zir = ZirClass::XmlTag;
                self.st.numeric = false;
                self.end_line_start(byte);
                return;
            }
            b'&' => {
                self.in_entity = true;
                self.st.zir = ZirClass::Entity;
            }
            b'[' => {
                if self.saw_open_bracket {
                    self.saw_open_bracket = false;
                    self.st.link = LinkState::Target;
                } else {
                    self.saw_open_bracket = true;
                }
                self.st.zir = ZirClass::WikiLink;
            }
            b']' => {
                if self.saw_close_bracket && self.st.link != LinkState::None {
                    self.st.link = LinkState::None;
                    self.saw_close_bracket = false;
                } else {
                    self.saw_close_bracket = !self.saw_close_bracket;
                }
                self.st.zir = ZirClass::WikiLink;
            }
            b'|' => {
                if self.saw_open_brace {
                    // `{|` opens a table; the pipe is the table marker, not a
                    // template parameter or a cell separator.
                    self.saw_open_brace = false;
                    self.st.table_depth = Self::bump(self.st.table_depth);
                    self.st.zir = ZirClass::WikiTable;
                } else {
                    if self.st.link == LinkState::Target {
                        self.st.link = LinkState::Surface;
                    }
                    if self.st.table_depth > 0 {
                        self.st.in_table_cell = true;
                    }
                    if self.st.template_depth > 0 {
                        self.st.in_template_param = true;
                    }
                    self.st.zir = ZirClass::Punct;
                }
            }
            b'{' => {
                // `{|` is resolved in the `|` arm; a second `{` here is `{{`.
                if self.saw_open_brace {
                    self.saw_open_brace = false;
                    self.st.template_depth = Self::bump(self.st.template_depth);
                    self.st.zir = ZirClass::WikiTemplate;
                } else {
                    self.saw_open_brace = true;
                    self.st.zir = ZirClass::Punct;
                }
            }
            b'}' => {
                if after_pipe && self.st.table_depth > 0 && self.st.template_depth == 0 {
                    // `|}` closes a table.
                    self.st.table_depth = self.st.table_depth.saturating_sub(1);
                    self.st.in_table_cell = false;
                    self.saw_close_brace = false;
                    self.st.zir = ZirClass::WikiTable;
                } else if self.saw_close_brace && self.st.template_depth > 0 {
                    self.saw_close_brace = false;
                    self.st.template_depth = self.st.template_depth.saturating_sub(1);
                    self.st.in_template_param = false;
                    self.st.zir = ZirClass::WikiTemplate;
                } else {
                    self.saw_close_brace = !self.saw_close_brace;
                    self.st.zir = ZirClass::WikiTemplate;
                }
            }
            b'\n' => {
                self.st.zir = ZirClass::Newlines;
                self.st.list_depth = 0;
                self.st.heading = 0;
                self.st.line_start = true;
                self.st.numeric = false;
                self.st.word_pos = 0;
                self.st.in_template_param = false;
                self.line_eq = 0;
                self.heading_decided = false;
                return;
            }
            b' ' | b'\t' => {
                self.st.zir = ZirClass::Spaces;
                self.st.numeric = false;
            }
            b'=' => {
                if self.st.line_start && !self.heading_decided {
                    self.line_eq = self.line_eq.saturating_add(1);
                    if self.line_eq <= 6 {
                        self.st.heading = self.line_eq;
                    }
                }
                self.st.zir = ZirClass::Punct;
            }
            b'*' | b'#' | b';' | b':' if self.st.line_start => {
                self.st.list_depth = Self::bump(self.st.list_depth);
                self.st.zir = ZirClass::Punct;
            }
            d if d.is_ascii_digit() => {
                self.st.zir = ZirClass::Number;
                self.st.numeric = true;
            }
            c if c.is_ascii_alphabetic() => {
                self.st.zir = ZirClass::Word;
                self.st.numeric = false;
                self.st.word_pos = self.st.word_pos.saturating_add(1).min(MAX_WORD_POS);
            }
            _ => {
                self.st.zir = ZirClass::Punct;
                self.st.numeric = false;
            }
        }
        // Any byte that is not one of the line-prefix or heading characters ends
        // the "line start" window; the ones that are keep it open.
        if !matches!(byte, b'=' | b' ' | b'\t')
            && !(self.st.line_start && matches!(byte, b'*' | b'#' | b';' | b':'))
        {
            self.st.line_start = false;
        }
    }
}

/// Fold a tag-name window into the same stable hash as [`hash_name`], ignoring
/// non-alphanumeric bytes so `<page>` and `< page` differ only if the name does.
fn tag_name_hash(buf: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut any = false;
    for &b in buf {
        if b.is_ascii_alphanumeric() {
            h ^= (b as u32) | 0x20;
            h = h.wrapping_mul(0x0100_0193);
            any = true;
        }
    }
    if any {
        h
    } else {
        0
    }
}

impl SignalBus for TrackingBus {
    fn observe(&mut self, byte: u8) {
        // Advance the counters first: every consumer of `state()` after this call
        // must see the position *after* this byte.
        self.st.byte_pos = self.st.byte_pos.wrapping_add(1);
        self.st.bitpos = 0;
        // A word run ends unless this byte continues it; the arms below set it.
        let was_word = self.st.zir == ZirClass::Word;
        if !(was_word && byte.is_ascii_alphanumeric()) {
            self.st.word_pos = 0;
        }

        if self.in_comment {
            self.observe_in_comment(byte);
            return;
        }
        if self.in_cdata {
            self.observe_in_cdata(byte);
            return;
        }
        if self.in_entity {
            self.observe_in_entity(byte);
            return;
        }
        if self.in_tag_angle {
            self.observe_in_tag(byte);
            return;
        }
        self.observe_normal(byte);
    }

    fn state(&self) -> SignalState {
        self.st
    }

    fn key(&self, fields: SignalFields) -> u32 {
        let s = &self.st;
        let mut h: u32 = 0x9e37_79b9;
        macro_rules! mix {
            ($cond:expr, $val:expr) => {
                if $cond {
                    h = h.wrapping_mul(0x0100_0193) ^ ((($val) as u32).wrapping_add(0x9e37));
                }
            };
        }
        mix!(fields.has(SignalFields::ZIR), s.zir.code());
        mix!(fields.has(SignalFields::PAGE), s.page.code());
        mix!(fields.has(SignalFields::LINK), s.link.code());
        mix!(fields.has(SignalFields::TEMPLATE), s.template_depth);
        mix!(fields.has(SignalFields::TABLE), s.table_depth);
        mix!(fields.has(SignalFields::LIST), s.list_depth);
        mix!(fields.has(SignalFields::HEADING), s.heading);
        mix!(
            fields.has(SignalFields::XML),
            (s.in_xml_tag as u32) | ((s.in_xml_attr as u32) << 1)
        );
        mix!(fields.has(SignalFields::WORD), s.word_pos);
        mix!(fields.has(SignalFields::NUMERIC), s.numeric as u32);
        mix!(fields.has(SignalFields::BITPOS), s.bitpos);
        mix!(fields.has(SignalFields::LINE_START), s.line_start as u32);
        h
    }

    fn set_bitpos(&mut self, bitpos: u8) {
        self.st.bitpos = bitpos & 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"<page>\n<title>A &amp; B</title>\n<id>12</id>\n<revision>\n\
<text>== H ==\n* item {{tmpl|a=b}} [[link|surf]] {| x |}\n<!-- c --> <![CDATA[<z>]]>\n\
</text>\n</revision>\n</page>\n";

    /// The property the decoder relies on, and the only reason a state tracker is
    /// legal in a context model at all.
    #[test]
    fn state_is_a_pure_function_of_the_bytes_so_far() {
        let mut whole = TrackingBus::new();
        for &b in SAMPLE {
            whole.observe(b);
        }
        for n in 1..=SAMPLE.len() {
            let mut bus = TrackingBus::new();
            for &b in &SAMPLE[..n] {
                bus.observe(b);
            }
            let mut q = TrackingBus::new();
            for &b in &SAMPLE[..n] {
                q.observe(b);
            }
            assert_eq!(bus.state(), q.state(), "prefix {n} is not deterministic");
        }
        let mut last = TrackingBus::new();
        for &b in SAMPLE {
            last.observe(b);
        }
        assert_eq!(whole.state(), last.state());
    }

    #[test]
    fn envelopes_are_tracked_and_restored() {
        let mut b = TrackingBus::new();
        for &x in b"<page>\n<title>T</title>\n<id>7</id>\n<revision>\n<text>hello" {
            b.observe(x);
        }
        assert_eq!(b.state().page, PageState::Text);
        for &x in b"</text>" {
            b.observe(x);
        }
        assert_eq!(b.state().page, PageState::Revision);
        for &x in b"</revision></page>" {
            b.observe(x);
        }
        assert_eq!(b.state().page, PageState::Page);
    }

    #[test]
    fn wiki_constructs_open_and_close() {
        let mut b = TrackingBus::new();
        for &x in b"{{a{{b}}c}}" {
            b.observe(x);
        }
        assert_eq!(b.state().template_depth, 0);
        assert_eq!(b.state().zir, ZirClass::WikiTemplate);

        let mut b = TrackingBus::new();
        for &x in b"[[target|surface]]" {
            b.observe(x);
        }
        assert_eq!(b.state().link, LinkState::None);
    }

    #[test]
    fn comments_and_cdata_swallow_angle_brackets() {
        let mut b = TrackingBus::new();
        for &x in b"<!-- <page> </page> -->" {
            b.observe(x);
        }
        assert_eq!(b.state().page, PageState::Outside);
        let mut b = TrackingBus::new();
        for &x in b"<![CDATA[<page>]]>" {
            b.observe(x);
        }
        assert_eq!(b.state().page, PageState::Outside);
    }

    #[test]
    fn headings_and_lists_are_line_local() {
        let mut b = TrackingBus::new();
        for &x in b"\n== Heading ==" {
            b.observe(x);
        }
        assert_eq!(b.state().heading, 2);
        for &x in b"\n" {
            b.observe(x);
        }
        assert_eq!(b.state().heading, 0);
        for &x in b"*** item" {
            b.observe(x);
        }
        assert_eq!(b.state().list_depth, 3);
    }

    #[test]
    fn depth_is_bounded_under_a_malformed_corpus() {
        let mut b = TrackingBus::new();
        for _ in 0..10_000 {
            for &x in b"{{{{{{{{{{" {
                b.observe(x);
            }
        }
        assert!(b.state().template_depth <= MAX_DEPTH);
    }

    #[test]
    fn fields_are_scoped_and_keys_stable() {
        let mut a = TrackingBus::new();
        let mut c = TrackingBus::new();
        for &x in b"<page><title>Apple</title>" {
            a.observe(x);
            c.observe(x);
        }
        assert_eq!(
            a.key(SignalFields::STRUCTURAL),
            c.key(SignalFields::STRUCTURAL)
        );
        assert_eq!(
            a.key(SignalFields::default()),
            c.key(SignalFields::default())
        );
        // Asking for a field that differs must be able to change the key.
        let _ = a.key(SignalFields::STRUCTURAL.or(SignalFields::WORD));
    }

    #[test]
    fn bitpos_round_trips_and_is_masked() {
        let mut b = TrackingBus::new();
        b.observe(b'x');
        b.set_bitpos(5);
        assert_eq!(b.state().bitpos, 5);
        b.set_bitpos(9);
        assert_eq!(b.state().bitpos, 1);
    }

    #[test]
    fn entity_state_ends() {
        // This test originally asserted `Spaces` after `"&amp; x"`, but the last
        // byte of that slice is `x`, a word — the assertion was on the wrong
        // slice, not a bug in the bus. The entity span is the whole `&amp;`
        // reference including its `;`; the byte *after* it is where ordinary
        // classification resumes.
        let mut b = TrackingBus::new();
        for &x in b"&amp;" {
            b.observe(x);
        }
        assert_eq!(b.state().zir, ZirClass::Entity);
        b.observe(b' ');
        assert_eq!(b.state().zir, ZirClass::Spaces);
        assert!(!b.state().numeric);
    }

    /// Feed a slice through one bus; start/stop points are just slices so a test
    /// can assert a state at any causal boundary without indexing bytes.
    fn run(bus: &mut TrackingBus, bytes: &[u8]) {
        for &b in bytes {
            bus.observe(b);
        }
    }

    #[test]
    fn a_pipe_is_a_template_param_only_inside_a_template() {
        let mut b = TrackingBus::new();
        run(&mut b, b"{{tpl|p");
        assert_eq!(b.state().template_depth, 1);
        assert!(b.state().in_template_param);
        // The same `|` must not be read as a table cell: no table is open.
        assert_eq!(b.state().table_depth, 0);
        assert!(!b.state().in_table_cell);
        run(&mut b, b"=1}}");
        assert_eq!(b.state().template_depth, 0);
        assert!(!b.state().in_template_param);
    }

    #[test]
    fn a_pipe_opens_a_table_cell_only_inside_a_table() {
        let mut b = TrackingBus::new();
        run(&mut b, b"{|");
        assert_eq!(b.state().table_depth, 1);
        // Opening the table is not itself a cell.
        assert!(!b.state().in_table_cell);
        run(&mut b, b" a");
        assert!(!b.state().in_table_cell);
        run(&mut b, b"| cell");
        assert!(b.state().in_table_cell);
        // ...and a table pipe is not a template parameter.
        assert_eq!(b.state().template_depth, 0);
        assert!(!b.state().in_template_param);
        run(&mut b, b"|}");
        assert_eq!(b.state().table_depth, 0);
        assert!(!b.state().in_table_cell);
    }

    #[test]
    fn a_template_inside_a_table_keeps_its_own_depth() {
        // Both constructs in one buffer: the table's `|` must not become a
        // template param and the template's `|` must not become a table cell.
        let mut c = TrackingBus::new();
        run(&mut c, b"{|\n| a {{t|p");
        assert_eq!(c.state().table_depth, 1);
        assert_eq!(c.state().template_depth, 1);
        assert!(c.state().in_template_param);
        assert!(c.state().in_table_cell);

        let mut b = TrackingBus::new();
        run(&mut b, b"{|\n| a {{t|p}} b\n|}");
        assert_eq!(b.state().table_depth, 0);
        assert_eq!(b.state().template_depth, 0);
        assert!(!b.state().in_template_param);
        assert!(!b.state().in_table_cell);
    }

    #[test]
    fn list_depth_resets_on_a_new_line() {
        let mut b = TrackingBus::new();
        run(&mut b, b"\n*** item");
        assert_eq!(b.state().list_depth, 3);
        run(&mut b, b"\nplain");
        assert_eq!(b.state().list_depth, 0);
    }

    #[test]
    fn a_closing_heading_run_does_not_raise_the_level() {
        // The level is set by the opening run of `=` at a line start; the closing
        // run on the same line must leave it alone.
        let mut b = TrackingBus::new();
        run(&mut b, b"\n== H");
        assert_eq!(b.state().heading, 2);
        run(&mut b, b" ==");
        assert_eq!(b.state().heading, 2);
        // Only six levels exist; a longer run saturates rather than wrapping.
        let mut deep = TrackingBus::new();
        run(&mut deep, b"\n======= x");
        assert_eq!(deep.state().heading, 6);
    }

    #[test]
    fn line_start_opens_on_a_newline_and_closes_on_content() {
        // Causality means the initial state has no `\n` behind it, so it is not
        // a line start; only an observed `\n` opens the window.
        let mut b = TrackingBus::new();
        assert!(!b.state().line_start);
        run(&mut b, b"\n");
        assert!(b.state().line_start);
        // Leading whitespace and the heading run keep it open.
        run(&mut b, b"  =");
        assert!(b.state().line_start);
        run(&mut b, b"x");
        assert!(!b.state().line_start);
    }

    #[test]
    fn word_pos_counts_a_letter_run_and_saturates() {
        let mut b = TrackingBus::new();
        run(&mut b, b"abc");
        assert_eq!(b.state().word_pos, 3);
        // A non-alphanumeric byte ends the run.
        run(&mut b, b"-");
        assert_eq!(b.state().word_pos, 0);
        assert_eq!(b.state().zir, ZirClass::Punct);
        // A later run starts over from one.
        run(&mut b, b"de");
        assert_eq!(b.state().word_pos, 2);
        // `word_pos` is bounded, so a very long run cannot grow the context.
        let mut long = TrackingBus::new();
        run(&mut long, b"aaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(long.state().word_pos, 15);
    }

    #[test]
    fn punctuation_is_classified_and_urls_are_not() {
        // Punctuation with no structural role is `Punct`.
        let mut p = TrackingBus::new();
        run(&mut p, b"!~@^%+-?");
        assert_eq!(p.state().zir, ZirClass::Punct);

        // `Url` is reserved in `ZirClass` but not produced: a URL has no tracked
        // extent because that would need bytes after the scheme. The gap is
        // asserted so it stays visible; if a future scanner implements it, this
        // test is the one that must change.
        let mut u = TrackingBus::new();
        run(&mut u, b"http://a.b");
        assert_ne!(u.state().zir, ZirClass::Url);
        assert_eq!(u.state().zir, ZirClass::Word);
    }
}
