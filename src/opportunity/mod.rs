//! Phase 14.1: where the codelength actually lives.
//!
//! The contract is fixed in [`PHASE14_PLAN.md`](../../docs/PHASE14_PLAN.md) §4.5:
//!
//! * every coded byte lands in exactly one structural class and one predictor
//!   role, and the report asserts `Σ classes == total` and `Σ roles == total`,
//!   so an unclassified byte is a bug rather than a rounding error;
//! * structural classes are the §14.4 tree (executable, transform state,
//!   restoration state, XML, titles/ids, revision metadata, templates, template
//!   parameters, links, categories, references, tables, lists, numbers, lexical,
//!   punctuation, match residual, dictionary state, learned model, unclassified);
//! * predictor-role attribution is either a *labelled* responsibility share (the
//!   mixer's weight mass) or a genuine counterfactual (an ablated pass), and the
//!   two are never conflated.
//!
//! ## What is actually measured
//!
//! Both modes price a stream by what the real accepted predictor charges for it,
//! bit by bit, using [`crate::entropy::RangeEncoder`]'s ideal-cost accumulator.
//! Each coded *decision* (8 per byte) is charged to a caller-supplied class, so
//! the partition is exact by construction rather than by rounding.
//!
//! * **default** (no flag). `crate::archive::encode_tuned` codes a *transformed*
//!   stream (article reorder → structural hoist → word tokenizer), so a coded
//!   position does not map 1:1 to a raw corpus position. We do **not** fake an
//!   alignment: we classify each coded byte by the structure of the transformed
//!   stream itself, which is well-defined and honest. The §14.4 tree has no
//!   `whitespace` class, so separator bytes are counted under `punctuation` and
//!   the finer distinction is preserved in `breakdown`.
//! * **`--raw`**. Classify the *raw* corpus 1:1 with [`crate::ir::tokenize`], and
//!   code it with `Method::RawCm`. This is the structurally meaningful measurement
//!   (the full ZIR taxonomy is in `breakdown`); the default mode is the
//!   pipeline-accurate one.
//!
//! ## Predictor roles
//!
//! The predictor is a context mixer over many experts, and its per-expert weights
//! are private to `crate::context`. Rather than present a made-up share as a
//! measurement, roles are measured by **sequential ablated-pass counterfactuals**:
//! the expert roster is grown one role at a time (literal → order model → word →
//! word bigram → match → sse → learned correction → procedural), each step is a
//! real coding pass over the real stream, and a role's share is the measured
//! change in ideal codelength when it is added. The closing residual (rounding /
//! interaction) lands in `unclassified`, so `Σ roles == total` exactly. This is
//! the honest basis label; the report prints it verbatim.

use std::fs;

use crate::archive::{self, Method};
use crate::context::{Cm, CtxKind, ModelConfig};
use crate::entropy::RangeEncoder;
use crate::ir::{self, Kind};

/// Number of structural classes in the §14.4 tree.
const NCLASS: usize = Class::ALL.len();

/// The §14.4 structural-class tree. Exactly these names, and every coded byte
/// lands in exactly one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Class {
    Executable,
    TransformState,
    RestorationState,
    XmlStructure,
    TitlesIds,
    RevisionMetadata,
    Templates,
    TemplateParams,
    Links,
    Categories,
    References,
    Tables,
    Lists,
    Numbers,
    Lexical,
    Punctuation,
    MatchResidual,
    DictState,
    LearnedModel,
    Unclassified,
}

impl Class {
    /// The complete, ordered tree.
    pub const ALL: [Class; 20] = [
        Class::Executable,
        Class::TransformState,
        Class::RestorationState,
        Class::XmlStructure,
        Class::TitlesIds,
        Class::RevisionMetadata,
        Class::Templates,
        Class::TemplateParams,
        Class::Links,
        Class::Categories,
        Class::References,
        Class::Tables,
        Class::Lists,
        Class::Numbers,
        Class::Lexical,
        Class::Punctuation,
        Class::MatchResidual,
        Class::DictState,
        Class::LearnedModel,
        Class::Unclassified,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Class::Executable => "executable",
            Class::TransformState => "transform_state",
            Class::RestorationState => "restoration_state",
            Class::XmlStructure => "xml_structure",
            Class::TitlesIds => "titles_ids",
            Class::RevisionMetadata => "revision_metadata",
            Class::Templates => "templates",
            Class::TemplateParams => "template_params",
            Class::Links => "links",
            Class::Categories => "categories",
            Class::References => "references",
            Class::Tables => "tables",
            Class::Lists => "lists",
            Class::Numbers => "numbers",
            Class::Lexical => "lexical",
            Class::Punctuation => "punctuation",
            Class::MatchResidual => "match_residual",
            Class::DictState => "dict_state",
            Class::LearnedModel => "learned_model",
            Class::Unclassified => "unclassified",
        }
    }
}

/// The predictor-expert roles. Attribution is a *sequential ablated-pass
/// counterfactual*, never a share presented as a measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Role {
    Literal,
    Match,
    Word,
    WordBigram,
    OrderModel,
    Sse,
    LearnedCorrection,
    Procedural,
    Unclassified,
}

impl Role {
    pub const ALL: [Role; 9] = [
        Role::Literal,
        Role::Match,
        Role::Word,
        Role::WordBigram,
        Role::OrderModel,
        Role::Sse,
        Role::LearnedCorrection,
        Role::Procedural,
        Role::Unclassified,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Role::Literal => "literal",
            Role::Match => "match",
            Role::Word => "word",
            Role::WordBigram => "word_bigram",
            Role::OrderModel => "order_model",
            Role::Sse => "sse",
            Role::LearnedCorrection => "learned_correction",
            Role::Procedural => "procedural",
            Role::Unclassified => "unclassified",
        }
    }
}

/// The basis label printed in the report so a diagnostic is never mistaken for a
/// measurement.
pub const ROLE_BASIS: &str =
    "sequential ablated-pass counterfactual (literal = order-0 + always-on calibration \
     baseline; each later share is a measured marginal Δcodelength; order-dependent)";

/// One entry of the attribution report: a class, and the bytes the coder
/// actually charged for it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attribution {
    pub name: &'static str,
    pub coded_bytes: f64,
}

/// The oracle's per-class row: the measured cost, and the zeroth-order empirical
/// bound computed from that class's byte histogram. The two are different kinds
/// of quantity and are labelled as such (§14.5).
#[derive(Clone, Debug, PartialEq)]
pub struct Bound {
    pub name: &'static str,
    pub measured_bytes: f64,
    pub zeroth_order_bytes: f64,
}

impl Bound {
    /// The *measured* quantity: what the accepted coder actually charged.
    pub const MEASURED_LABEL: &'static str = "REALIZABLE (measured ideal codelength)";
    /// The *fitted* quantity: a zeroth-order model fit to the class's own byte
    /// histogram. It is a target, not a decoder-realizable bound.
    pub const BOUND_LABEL: &'static str = "TARGET-FITTED ENTROPY (zeroth-order, class histogram)";
}

/// The complete report, which is only evidence once its parts sum to its total.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub total_coded_bytes: f64,
    pub structural: Vec<Attribution>,
    pub roles: Vec<Attribution>,
    /// Rendered once so a caller does not re-derive the table shape.
    pub lines: Vec<String>,
    /// The classification mode this report was produced in.
    pub mode: String,
    /// How the role partition was measured (see [`ROLE_BASIS`]).
    pub role_basis: String,
    /// The honest limitation of the mode, printed in the header.
    pub limitation: String,
    /// A finer, mode-specific taxonomy (ZIR kinds for `--raw`, transform-level
    /// buckets for the default mode). Its coded bytes also sum to the total.
    pub breakdown: Vec<Attribution>,
    /// The oracle rows, empty unless `--oracle` was passed.
    pub bounds: Vec<Bound>,
    /// The container payload length the coding pass produced, as a cross-check
    /// against the ideal total.
    pub actual_coded_bytes: usize,
}

impl Report {
    /// True when both partitions sum to the total within floating-point slack.
    pub fn is_attributable(&self) -> bool {
        let s: f64 = self.structural.iter().map(|a| a.coded_bytes).sum();
        let r: f64 = self.roles.iter().map(|a| a.coded_bytes).sum();
        (s - self.total_coded_bytes).abs() <= 1.0 && (r - self.total_coded_bytes).abs() <= 1.0
    }

    pub fn render(&self) -> String {
        self.lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Options {
    raw: bool,
    oracle: bool,
    json: bool,
    top: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            raw: false,
            oracle: false,
            json: false,
            top: usize::MAX,
        }
    }
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--raw" => o.raw = true,
            "--oracle" => o.oracle = true,
            "--json" => o.json = true,
            "--top" => {
                i += 1;
                let v = args.get(i).ok_or("opportunity: --top needs a value")?;
                o.top = v
                    .parse::<usize>()
                    .map_err(|_| format!("opportunity: bad --top value: {v}"))?;
            }
            s if s.starts_with("--top=") => {
                let v = &s["--top=".len()..];
                o.top = v
                    .parse::<usize>()
                    .map_err(|_| format!("opportunity: bad --top value: {v}"))?;
            }
            other => return Err(format!("opportunity: unknown flag: {other}")),
        }
        i += 1;
    }
    Ok(o)
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// `zentropy opportunity <corpus>` — the entry point the driver calls.
///
/// Returns `Err` rather than a report whose partitions do not balance, so an
/// unattributed byte can never be read as a measurement.
pub fn run(path: &str, args: &[String]) -> Result<Report, String> {
    let opts = parse_args(args)?;
    let data = fs::read(path).map_err(|e| format!("opportunity: cannot read {path}: {e}"))?;
    let mut report = if opts.raw {
        analyze_raw(&data)?
    } else {
        analyze_default(&data)?
    };
    if !report.is_attributable() {
        let s: f64 = report.structural.iter().map(|a| a.coded_bytes).sum();
        let r: f64 = report.roles.iter().map(|a| a.coded_bytes).sum();
        return Err(format!(
            "opportunity: attribution does not balance — total={:.6} structural={:.6} roles={:.6}",
            report.total_coded_bytes, s, r
        ));
    }
    render(&mut report, &opts);
    Ok(report)
}

// ---------------------------------------------------------------------------
// The coding pass: price a stream, charging every decision to a class
// ---------------------------------------------------------------------------

/// Code `stream` with an already-tuned `cfg`, charging each byte's 8 decisions to
/// the class index `class_of[i]` (`class_of.len() == stream.len()`).
///
/// Returns `(ideal_bits, per_class_bits, payload)`. The payload is byte-identical
/// to the one `crate::archive::encode_tuned` would emit for the same config and
/// stream — a property the tests assert, which is what keeps the attribution
/// honest about *which* stream it measured.
fn code_classified(
    cfg: &ModelConfig,
    stream: &[u8],
    class_of: &[usize],
    n_classes: usize,
) -> (f64, Vec<f64>, Vec<u8>) {
    debug_assert_eq!(stream.len(), class_of.len());
    let mut cm = Cm::new(cfg, stream.len());
    let mut enc = RangeEncoder::with_capacity(stream.len() / 2 + 64);
    enc.set_class_count(n_classes);
    for (i, &byte) in stream.iter().enumerate() {
        enc.set_class(class_of[i]);
        let mut mask = 0x80u32;
        while mask != 0 {
            let bit = if (byte as u32) & mask != 0 { 1 } else { 0 };
            let p = cm.predict();
            enc.encode(bit, p);
            cm.update(bit);
            mask >>= 1;
        }
    }
    let bits = enc.cost_bits();
    let costs = enc.class_costs().to_vec();
    (bits, costs, enc.finish())
}

/// Ideal codelength in bits of coding `stream` with an already-tuned `cfg`.
fn code_total(cfg: &ModelConfig, stream: &[u8]) -> f64 {
    let mut cm = Cm::new(cfg, stream.len());
    let mut enc = RangeEncoder::with_capacity(stream.len() / 2 + 64);
    for &byte in stream {
        let mut mask = 0x80u32;
        while mask != 0 {
            let bit = if (byte as u32) & mask != 0 { 1 } else { 0 };
            let p = cm.predict();
            enc.encode(bit, p);
            cm.update(bit);
            mask >>= 1;
        }
    }
    enc.cost_bits()
}

// ---------------------------------------------------------------------------
// Breakdown bookkeeping
// ---------------------------------------------------------------------------

/// A dynamic, mode-specific taxonomy finer than the structural tree. Each entry
/// belongs to exactly one structural class, so aggregating entry costs recovers
/// the structural partition exactly.
struct BdSet {
    table: Vec<(&'static str, Class)>,
}

impl BdSet {
    fn new() -> Self {
        BdSet { table: Vec::new() }
    }

    fn id(&mut self, name: &'static str, class: Class) -> usize {
        if let Some(i) = self.table.iter().position(|&(n, _)| n == name) {
            i
        } else {
            self.table.push((name, class));
            self.table.len() - 1
        }
    }
}

fn add_attr(v: &mut Vec<Attribution>, name: &'static str, bytes: f64) {
    match v.iter_mut().find(|a| a.name == name) {
        Some(a) => a.coded_bytes += bytes,
        None => v.push(Attribution {
            name,
            coded_bytes: bytes,
        }),
    }
}

// ---------------------------------------------------------------------------
// Raw mode: 1:1 corpus attribution
// ---------------------------------------------------------------------------

/// Map a ZIR token kind to the §14.4 structural class. The full ZIR taxonomy is
/// preserved in the report's `breakdown`.
fn kind_class(k: Kind) -> Class {
    match k {
        Kind::XmlOpen
        | Kind::XmlClose
        | Kind::XmlEmpty
        | Kind::XmlComment
        | Kind::XmlPi
        | Kind::Cdata
        // An HTML/XML entity is a markup-level escape, not prose.
        | Kind::Entity => Class::XmlStructure,
        Kind::Template => Class::Templates,
        Kind::WikiLink | Kind::Url => Class::Links,
        Kind::WikiTable => Class::Tables,
        Kind::Word => Class::Lexical,
        Kind::Number => Class::Numbers,
        // The §14.4 tree has no whitespace class; separator bytes are reported
        // under `punctuation` and kept distinct in `breakdown`.
        Kind::Spaces | Kind::Newlines | Kind::Punct => Class::Punctuation,
        Kind::Raw => Class::Unclassified,
    }
}

/// Classify a raw corpus 1:1 by ZIR kind, returning per-byte breakdown ids, the
/// breakdown table, and the per-structural-class byte histograms (for `--oracle`).
fn classify_raw(corpus: &[u8]) -> (Vec<usize>, BdSet, Vec<[u64; 256]>) {
    let mut bd = BdSet::new();
    let mut class_of = vec![Class::Unclassified as usize; corpus.len()];
    let mut hist = vec![[0u64; 256]; NCLASS];
    // Pre-register so the breakdown table order is the stable ZIR `Kind::ALL`.
    for k in Kind::ALL {
        bd.id(k.name(), kind_class(k));
    }
    for tok in ir::tokenize(corpus) {
        let class = kind_class(tok.kind);
        let id = bd.id(tok.kind.name(), class);
        for k in tok.range() {
            class_of[k] = id;
            hist[class as usize][corpus[k] as usize] += 1;
        }
    }
    (class_of, bd, hist)
}

fn analyze_raw(data: &[u8]) -> Result<Report, String> {
    // `RawCm` applies no transforms, so the coded stream is the corpus itself;
    // we still ask the archive for it so the alignment claim is structural rather
    // than assumed.
    let stream = archive::transformed_stream(data, Method::RawCm, 0);
    if stream.len() != data.len() {
        return Err(format!(
            "opportunity --raw: RawCm transformed {} bytes from {} (expected 1:1)",
            stream.len(),
            data.len()
        ));
    }
    let (class_of, bd, hist) = classify_raw(&stream);
    let cfg = Method::RawCm.config_for(stream.len()).with_tune(0);
    let (bits, bd_costs, payload) = code_classified(&cfg, &stream, &class_of, bd.table.len());

    let mut report = Report {
        total_coded_bytes: bits / 8.0,
        mode: "raw (corpus 1:1, Method::RawCm)".into(),
        role_basis: ROLE_BASIS.into(),
        limitation: "RawCm applies no transforms, so coded positions map 1:1 to corpus \
                     positions. The ZIR taxonomy is exact, but the classes are structural \
                     spans of the raw corpus, not of the pipeline's transformed stream."
            .into(),
        actual_coded_bytes: payload.len(),
        ..Default::default()
    };
    finish_partitions(&mut report, &bd, &bd_costs);
    report.roles = role_partition(&cfg, &stream, bits);
    report.bounds = oracle_bounds(&report, &hist);
    Ok(report)
}

// ---------------------------------------------------------------------------
// Default mode: the accepted pipeline's coded stream
// ---------------------------------------------------------------------------

/// The method the accepted pipeline's outermost reorder actually leaves in
/// force: `Method::Residual` unless the free-restoration precondition fails and
/// `prepare_reorder` downgrades to `Sse3`. We mirror the encoder's decision here
/// so the config we price with is the config it used.
fn effective_method(input: &[u8]) -> Method {
    #[cfg(feature = "reorder")]
    {
        if crate::reorder::encode(input, crate::reorder::Order::Full).is_none() {
            return Method::Sse3;
        }
    }
    Method::Residual
}

/// Classify the *transformed* stream produced by the accepted pipeline. The
/// structural-hoist transform emits single control bytes in `0x01..=0x1F` for
/// dictionary strings and `0x00, byte` for reserved literals; the word tokenizer
/// then emits a vocabulary header followed by a body in which `0x00, id` is a
/// token reference (`id == 0` is a literal NUL) and everything else is copied.
fn classify_transformed(data: &[u8]) -> (Vec<usize>, BdSet, Vec<[u64; 256]>) {
    let n = data.len();
    let mut bd = BdSet::new();
    let mut class_of = vec![Class::Unclassified as usize; n];
    let mut hist = vec![[0u64; 256]; NCLASS];
    let mut entries: Vec<(usize, usize)> = Vec::new();

    let mut i = 0usize;
    // Vocabulary header: u8 count, then `u8 len || word` per entry.
    if n > 0 {
        let count = data[0] as usize;
        {
            let id = bd.id("dict_count", Class::DictState);
            class_of[0] = id;
            hist[Class::DictState as usize][data[0] as usize] += 1;
        }
        i = 1;
        for _ in 0..count {
            if i >= n {
                break;
            }
            let l = data[i] as usize;
            let len_id = bd.id("dict_header", Class::DictState);
            class_of[i] = len_id;
            hist[Class::DictState as usize][data[i] as usize] += 1;
            i += 1;
            let end = (i + l).min(n);
            let word_id = bd.id("dict_entry", Class::DictState);
            for k in i..end {
                class_of[k] = word_id;
                hist[Class::DictState as usize][data[k] as usize] += 1;
            }
            entries.push((i, end));
            i = end;
        }
    }

    // Body.
    while i < n {
        let b = data[i];
        if b == crate::transform::TOK_ESC {
            let id = bd.id("token_control", Class::Lexical);
            class_of[i] = id;
            hist[Class::Lexical as usize][b as usize] += 1;
            if i + 1 < n {
                let tok = data[i + 1];
                let (class, label) = if tok == 0 {
                    (Class::Unclassified, "escape_literal")
                } else {
                    let is_word = entries.get(tok as usize - 1).map_or(false, |&(s, e)| {
                        data[s..e]
                            .iter()
                            .all(|&x| crate::transform::is_word_byte_at(x))
                    });
                    if is_word {
                        (Class::Lexical, "word_ref")
                    } else {
                        (Class::Punctuation, "nonword_ref")
                    }
                };
                let id = bd.id(label, class);
                class_of[i + 1] = id;
                hist[class as usize][tok as usize] += 1;
                i += 2;
            } else {
                i += 1;
            }
        } else if (1..=0x1F).contains(&b) {
            let id = bd.id("hoist_code", Class::XmlStructure);
            class_of[i] = id;
            hist[Class::XmlStructure as usize][b as usize] += 1;
            i += 1;
        } else {
            let (class, label) = if b.is_ascii_digit() {
                (Class::Numbers, "digit")
            } else if b.is_ascii_alphabetic() {
                (Class::Lexical, "letter")
            } else if b.is_ascii_whitespace() {
                (Class::Punctuation, "whitespace")
            } else if b.is_ascii() {
                (Class::Punctuation, "punct")
            } else {
                (Class::Unclassified, "other")
            };
            let id = bd.id(label, class);
            class_of[i] = id;
            hist[class as usize][data[i] as usize] += 1;
            i += 1;
        }
    }
    (class_of, bd, hist)
}

fn analyze_default(data: &[u8]) -> Result<Report, String> {
    let method = effective_method(data);
    let stream = archive::transformed_stream(data, method, archive::ACCEPTED_TUNE);
    let (class_of, bd, hist) = classify_transformed(&stream);
    let cfg = method
        .config_for(stream.len())
        .with_tune(archive::ACCEPTED_TUNE);
    let (bits, bd_costs, payload) = code_classified(&cfg, &stream, &class_of, bd.table.len());

    let mut report = Report {
        total_coded_bytes: bits / 8.0,
        mode: format!("default (coded transformed stream, Method::{method:?})"),
        role_basis: ROLE_BASIS.into(),
        limitation: "The accepted pipeline codes a transformed stream (reorder -> hoist -> \
                     word tokenize), so coded positions do not map 1:1 to corpus positions. \
                     Each coded byte is classified by the transformed stream's own structure; \
                     the §14.4 tree has no whitespace class, so separators are counted under \
                     punctuation and shown apart in `breakdown`. The archive header and \
                     executable/model bytes are not part of this codelength measurement."
            .into(),
        actual_coded_bytes: payload.len(),
        ..Default::default()
    };
    finish_partitions(&mut report, &bd, &bd_costs);
    report.roles = role_partition(&cfg, &stream, bits);
    report.bounds = oracle_bounds(&report, &hist);
    Ok(report)
}

// ---------------------------------------------------------------------------
// Partitions
// ---------------------------------------------------------------------------

/// Build the structural and breakdown partitions from per-breakdown coded costs.
fn finish_partitions(report: &mut Report, bd: &BdSet, bd_costs: &[f64]) {
    // Breakdown rows, in coding order.
    for (i, &(name, _)) in bd.table.iter().enumerate() {
        let bytes = bd_costs.get(i).copied().unwrap_or(0.0) / 8.0;
        if bytes > 0.0 {
            add_attr(&mut report.breakdown, name, bytes);
        }
    }
    // Structural rows: sum each breakdown's cost into its class.
    let mut per_class = vec![0.0f64; NCLASS];
    for (i, &(_, class)) in bd.table.iter().enumerate() {
        per_class[class as usize] += bd_costs.get(i).copied().unwrap_or(0.0);
    }
    for c in Class::ALL {
        let bytes = per_class[c as usize] / 8.0;
        if bytes > 0.0 {
            add_attr(&mut report.structural, c.name(), bytes);
        }
    }
    report
        .structural
        .sort_by(|a, b| b.coded_bytes.partial_cmp(&a.coded_bytes).unwrap());
    report
        .breakdown
        .sort_by(|a, b| b.coded_bytes.partial_cmp(&a.coded_bytes).unwrap());
}

/// The predictor roles an expert belongs to.
fn role_of_spec(kind: CtxKind) -> Role {
    match kind {
        CtxKind::Order(0) => Role::Literal,
        CtxKind::Word | CtxKind::WordClass | CtxKind::WordClassConst => Role::Word,
        CtxKind::Stem | CtxKind::StemCtl => Role::Word,
        CtxKind::WordBigram => Role::WordBigram,
        CtxKind::MatchByte | CtxKind::MatchByteConst => Role::Match,
        // Everything else is a byte/structure context model.
        CtxKind::Order(_)
        | CtxKind::Column
        | CtxKind::ColumnShuffled
        | CtxKind::ColumnNoLine
        | CtxKind::StateOrder(_)
        | CtxKind::Sparse(_, _)
        | CtxKind::Indirect { .. }
        | CtxKind::IndirectCtl { .. } => Role::OrderModel,
    }
}

/// A config containing only the listed specs, with every non-spec expert (match
/// tiers, the extra SSE stage, the learned corrector, PPM) turned off. One
/// cumulative config is built per role step of the ablation ladder.
fn ablate(cfg: &ModelConfig, keep: &[usize]) -> ModelConfig {
    let mut c = cfg.ablated(keep);
    c.matches = Vec::new();
    c.rep_offsets = 0;
    c.apm3_ctx = 0;
    c.residual = false;
    c.residual_ctl = false;
    c.ppm_order = 0;
    c
}

/// Measure each role by adding it to the roster one step at a time and charging
/// the *change* in ideal codelength to it. The closing residual (rounding and
/// interaction) lands in `unclassified`, so the partition is exact.
///
/// Each rung of the ladder is a subset of the real roster **in its original
/// order**, so the final rung is byte-identical to the accepted configuration and
/// the counterfactual is anchored to the pipeline it claims to explain.
fn role_partition(cfg: &ModelConfig, stream: &[u8], full_bits: f64) -> Vec<Attribution> {
    // The ladder order: broadest explanation first, narrowest last.
    let order = [
        Role::Literal,
        Role::OrderModel,
        Role::Word,
        Role::WordBigram,
        Role::Match,
        Role::Sse,
        Role::LearnedCorrection,
        Role::Procedural,
    ];
    let rank = |r: Role| order.iter().position(|&x| x == r).unwrap();

    let mut costs: Vec<f64> = Vec::with_capacity(order.len());
    for &reached in &order {
        let max = rank(reached);
        // Keep the specs whose role is at or below this rung, in *original* order
        // so the surviving roster is a subsequence of the accepted one.
        let keep: Vec<usize> = (0..cfg.specs.len())
            .filter(|&i| rank(role_of_spec(cfg.specs[i].kind)) <= max)
            .collect();
        let mut c = ablate(cfg, &keep);
        // Restore non-spec experts once their rung is reached.
        if max >= rank(Role::OrderModel) {
            c.ppm_order = cfg.ppm_order;
        }
        if max >= rank(Role::Match) {
            c.matches = cfg.matches.clone();
            c.rep_offsets = cfg.rep_offsets;
        }
        if max >= rank(Role::Sse) {
            c.apm3_ctx = cfg.apm3_ctx;
            c.apm3_mode = cfg.apm3_mode;
            c.apm3_rate = cfg.apm3_rate;
        }
        if max >= rank(Role::LearnedCorrection) {
            c.residual = cfg.residual;
            c.residual_ctl = cfg.residual_ctl;
        }
        costs.push(code_total(&c, stream));
    }

    // literal is the baseline cost; each later role is the marginal change.
    let mut share = vec![0.0f64; Role::ALL.len()];
    share[Role::Literal as usize] = costs[0];
    for step in 1..order.len() {
        share[order[step] as usize] = costs[step] - costs[step - 1];
    }
    // Closing residual: what the sequence did not attribute, kept explicit.
    let attributed: f64 = share.iter().sum();
    share[Role::Unclassified as usize] = full_bits - attributed;

    let mut out: Vec<Attribution> = Role::ALL
        .iter()
        .map(|&r| Attribution {
            name: r.name(),
            coded_bytes: share[r as usize] / 8.0,
        })
        .collect();
    out.sort_by(|a, b| b.coded_bytes.partial_cmp(&a.coded_bytes).unwrap());
    out
}

// ---------------------------------------------------------------------------
// The oracle: zeroth-order empirical bounds, clearly separated
// ---------------------------------------------------------------------------

/// Zeroth-order entropy in bits of a byte histogram: `H0 * n`.
fn zeroth_order_bits(hist: &[u64; 256]) -> f64 {
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return 0.0;
    }
    let mut h = 0.0f64;
    for &c in hist {
        if c > 0 {
            let p = c as f64 / total as f64;
            h -= p * p.log2();
        }
    }
    h * total as f64
}

fn oracle_bounds(report: &Report, hist: &[[u64; 256]]) -> Vec<Bound> {
    report
        .structural
        .iter()
        .map(|a| {
            let class = Class::ALL
                .iter()
                .find(|c| c.name() == a.name)
                .copied()
                .unwrap_or(Class::Unclassified);
            Bound {
                name: a.name,
                measured_bytes: a.coded_bytes,
                zeroth_order_bytes: zeroth_order_bits(&hist[class as usize]) / 8.0,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn pct(part: f64, total: f64) -> f64 {
    if total > 0.0 {
        100.0 * part / total
    } else {
        0.0
    }
}

fn render(report: &mut Report, opts: &Options) {
    if opts.json {
        render_json(report, opts);
    } else {
        render_text(report, opts);
    }
}

fn render_text(report: &mut Report, opts: &Options) {
    let mut out_lines: Vec<String> = Vec::new();
    out_lines.push(format!("zentropy opportunity — mode: {}", report.mode));
    out_lines.push(format!("  limitation: {}", report.limitation));
    out_lines.push(format!(
        "  total coded: {:.3} B (ideal); container payload {} B",
        report.total_coded_bytes, report.actual_coded_bytes
    ));
    out_lines.push(format!("  roles measured by: {}", report.role_basis));

    let top = opts.top;

    out_lines.push("structural (Σ == total):".into());
    for a in report.structural.iter().take(top) {
        out_lines.push(format!(
            "  {:<18} {:>14.3} B  {:>6.2}%",
            a.name,
            a.coded_bytes,
            pct(a.coded_bytes, report.total_coded_bytes)
        ));
    }
    out_lines.push("breakdown (Σ == total):".into());
    for a in report.breakdown.iter().take(top) {
        out_lines.push(format!(
            "  {:<18} {:>14.3} B  {:>6.2}%",
            a.name,
            a.coded_bytes,
            pct(a.coded_bytes, report.total_coded_bytes)
        ));
    }
    out_lines.push(format!("roles [{}]:", report.role_basis));
    for a in report.roles.iter().take(top) {
        out_lines.push(format!(
            "  {:<18} {:>14.3} B  {:>6.2}%",
            a.name,
            a.coded_bytes,
            pct(a.coded_bytes, report.total_coded_bytes)
        ));
    }
    if opts.oracle && !report.bounds.is_empty() {
        out_lines.push(format!(
            "oracle per class — measured = {} | bound = {}:",
            Bound::MEASURED_LABEL,
            Bound::BOUND_LABEL
        ));
        for b in report.bounds.iter().take(top) {
            out_lines.push(format!(
                "  {:<18} measured {:>14.3} B | zeroth-order {:>14.3} B",
                b.name, b.measured_bytes, b.zeroth_order_bytes
            ));
        }
    }
    let s: f64 = report.structural.iter().map(|a| a.coded_bytes).sum();
    let r: f64 = report.roles.iter().map(|a| a.coded_bytes).sum();
    out_lines.push(format!(
        "attributable: structural_sum={s:.3} role_sum={r:.3} total={:.3}",
        report.total_coded_bytes
    ));
    report.lines = out_lines;
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn render_json(report: &mut Report, opts: &Options) {
    let mut out_lines: Vec<String> = Vec::new();
    out_lines.push(format!(
        "{{\"kind\":\"header\",\"mode\":\"{}\",\"limitation\":\"{}\",\"role_basis\":\"{}\",\
         \"total_coded_bytes\":{:.6},\"container_payload_bytes\":{},\"attributable\":true}}",
        json_escape(&report.mode),
        json_escape(&report.limitation),
        json_escape(&report.role_basis),
        report.total_coded_bytes,
        report.actual_coded_bytes
    ));
    for a in report.structural.iter().take(opts.top) {
        out_lines.push(format!(
            "{{\"kind\":\"structural\",\"name\":\"{}\",\"coded_bytes\":{:.6}}}",
            a.name, a.coded_bytes
        ));
    }
    for a in report.breakdown.iter().take(opts.top) {
        out_lines.push(format!(
            "{{\"kind\":\"breakdown\",\"name\":\"{}\",\"coded_bytes\":{:.6}}}",
            a.name, a.coded_bytes
        ));
    }
    for a in report.roles.iter().take(opts.top) {
        out_lines.push(format!(
            "{{\"kind\":\"role\",\"name\":\"{}\",\"coded_bytes\":{:.6}}}",
            a.name, a.coded_bytes
        ));
    }
    for b in report
        .bounds
        .iter()
        .take(if opts.oracle { opts.top } else { 0 })
    {
        out_lines.push(format!(
            "{{\"kind\":\"oracle\",\"name\":\"{}\",\"measured_bytes\":{:.6},\
             \"zeroth_order_bytes\":{:.6},\"measured_label\":\"{}\",\"bound_label\":\"{}\"}}",
            b.name,
            b.measured_bytes,
            b.zeroth_order_bytes,
            json_escape(Bound::MEASURED_LABEL),
            json_escape(Bound::BOUND_LABEL)
        ));
    }
    report.lines = out_lines;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<u8> {
        // Small but structurally varied: pages, ids, a template, a link, a table,
        // an entity, numbers, punctuation and whitespace.
        let mut v = Vec::new();
        for i in 1..=8u32 {
            v.extend_from_slice(
                format!(
                    "<page>\n  <title>Article {i}</title>\n  <id>{i}</id>\n  \
                     <revision><timestamp>2020-01-0{i}T00:00:00Z</timestamp></revision>\n  \
                     <text>the quick brown fox jumps over the lazy dog \
                     {{cite web|url=http://example.org/{i}|title=Fox}} \
                     [[Link {i}]] &amp; 12345\n{{| class=\"wikitable\"\n|-\n| a || b\n|}}\n</text>\n</page>\n"
                )
                .as_bytes(),
            );
        }
        v
    }

    #[test]
    fn default_report_is_attributable_and_reproduces_payload() {
        let data = sample();
        let method = effective_method(&data);
        let stream = archive::transformed_stream(&data, method, archive::ACCEPTED_TUNE);
        let (class_of, bd, _hist) = classify_transformed(&stream);
        let cfg = method
            .config_for(stream.len())
            .with_tune(archive::ACCEPTED_TUNE);
        let (bits, _costs, payload) = code_classified(&cfg, &stream, &class_of, bd.table.len());

        // The coding pass must be byte-identical to the real encoder's payload:
        // that is what makes the attribution a measurement of *this* pipeline.
        let arch = archive::encode_tuned(&data, method, archive::ACCEPTED_TUNE);
        assert_eq!(
            payload.as_slice(),
            &arch[archive::HEADER_LEN..],
            "attribution pass diverged from encode_tuned"
        );

        let report = analyze_default(&data).expect("analyze");
        assert!(report.is_attributable());
        assert!((report.total_coded_bytes - bits / 8.0).abs() < 1e-6);
        assert!(report.mode.contains("default"));
        // A dictionary was built, so dictionary state must be present.
        assert!(
            report.structural.iter().any(|a| a.name == "dict_state"),
            "expected dict_state in {:?}",
            report.structural
        );
        assert!(report.roles.iter().all(|a| !a.name.is_empty()));
    }

    #[test]
    fn raw_report_is_attributable_and_reproduces_payload() {
        let data = sample();
        let stream = archive::transformed_stream(&data, Method::RawCm, 0);
        assert_eq!(stream, data, "RawCm must not transform");
        let (class_of, bd, _hist) = classify_raw(&stream);
        let cfg = Method::RawCm.config_for(stream.len()).with_tune(0);
        let (bits, _costs, payload) = code_classified(&cfg, &stream, &class_of, bd.table.len());
        let arch = archive::encode_with(&data, Method::RawCm);
        assert_eq!(payload.as_slice(), &arch[archive::HEADER_LEN..]);

        let report = analyze_raw(&data).expect("analyze");
        assert!(report.is_attributable());
        assert!((report.total_coded_bytes - bits / 8.0).abs() < 1e-6);
        // The ZIR taxonomy is preserved in the breakdown.
        assert!(report.breakdown.iter().any(|a| a.name == "xml_open"));
        assert!(report.breakdown.iter().any(|a| a.name == "word"));
        assert!(report.breakdown.iter().any(|a| a.name == "spaces"));
    }

    #[test]
    fn partitions_sum_exactly() {
        for raw in [false, true] {
            let data = sample();
            let opts = Options {
                raw,
                oracle: true,
                json: false,
                top: usize::MAX,
            };
            let mut report = if raw {
                analyze_raw(&data).unwrap()
            } else {
                analyze_default(&data).unwrap()
            };
            let s: f64 = report.structural.iter().map(|a| a.coded_bytes).sum();
            let r: f64 = report.roles.iter().map(|a| a.coded_bytes).sum();
            let b: f64 = report.breakdown.iter().map(|a| a.coded_bytes).sum();
            assert!(
                (s - report.total_coded_bytes).abs() <= 1.0,
                "structural {s}"
            );
            assert!((r - report.total_coded_bytes).abs() <= 1.0, "roles {r}");
            assert!((b - report.total_coded_bytes).abs() <= 1.0, "breakdown {b}");
            // Oracle rows mirror the structural partition.
            let om: f64 = report.bounds.iter().map(|x| x.measured_bytes).sum();
            assert!((om - report.total_coded_bytes).abs() <= 1.0);
            assert!(report
                .bounds
                .iter()
                .all(|x| x.zeroth_order_bytes.is_finite()));
            render(&mut report, &opts);
            assert!(!report.render().is_empty());
        }
    }

    #[test]
    fn is_attributable_detects_mismatch() {
        let bad = Report {
            total_coded_bytes: 100.0,
            structural: vec![Attribution {
                name: "lexical",
                coded_bytes: 10.0,
            }],
            roles: vec![Attribution {
                name: "literal",
                coded_bytes: 100.0,
            }],
            ..Default::default()
        };
        assert!(!bad.is_attributable());
    }

    #[test]
    fn flag_parsing() {
        let o = parse_args(&[
            "--raw".into(),
            "--oracle".into(),
            "--json".into(),
            "--top".into(),
            "5".into(),
        ])
        .unwrap();
        assert!(o.raw && o.oracle && o.json && o.top == 5);
        let o = parse_args(&["--top=3".into()]).unwrap();
        assert_eq!(o.top, 3);
        assert!(parse_args(&["--nope".into()]).is_err());
    }

    #[test]
    fn json_render_is_line_delimited() {
        let data = sample();
        let mut report = analyze_raw(&data).unwrap();
        let opts = Options {
            raw: true,
            oracle: true,
            json: true,
            top: 4,
        };
        render(&mut report, &opts);
        for line in &report.lines {
            assert!(line.starts_with('{') && line.ends_with('}'));
            assert!(line.contains("\"kind\":"));
        }
        assert!(report
            .lines
            .iter()
            .any(|l| l.contains("\"kind\":\"oracle\"")));
    }

    /// The real measurement on the development rung. Ignored by default because
    /// it runs the accepted predictor eight times over a megabyte; run it with
    /// `cargo test --release --features opportunity opportunity::tests::enwik6 -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn enwik6_smoke() {
        let path = "evidence/corpus/enwik6";
        if !std::path::Path::new(path).exists() {
            eprintln!("enwik6_smoke: {path} not present; skipping");
            return;
        }
        let args: Vec<String> = vec!["--top".into(), "12".into(), "--oracle".into()];
        let report = run(path, &args).expect("opportunity run");
        assert!(report.is_attributable());
        println!("{}", report.render());
        let sum: f64 = report.structural.iter().map(|a| a.coded_bytes).sum();
        assert!((sum - report.total_coded_bytes).abs() <= 1.0);
    }

    /// The raw, 1:1, structurally meaningful measurement on the development rung.
    #[test]
    #[ignore]
    fn enwik6_raw_smoke() {
        let path = "evidence/corpus/enwik6";
        if !std::path::Path::new(path).exists() {
            eprintln!("enwik6_raw_smoke: {path} not present; skipping");
            return;
        }
        let args: Vec<String> = vec!["--raw".into(), "--top".into(), "20".into()];
        let report = run(path, &args).expect("opportunity run --raw");
        assert!(report.is_attributable());
        println!("{}", report.render());
    }
}
