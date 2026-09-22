//! Phase 14.29: the explanation is data, and it must be compressed.
//!
//! **Why this module exists.** The P14.9 boundary experiment charged every
//! program at `serialize(program).len()` — raw varints — and on the `template`
//! class that term was **14,097 B of a 24,361 B** candidate cost (58%). The
//! experiment therefore never tested the family as specified: §14.29 requires the
//! program to be split into streams and modelled like any other data, and §14.28
//! requires each model to pay for its own persisted bytes. This module is the
//! program-side entropy coder that makes the explanation pay for itself.
//!
//! ## The split (§14.29)
//!
//! Each field of the serialized program lands in exactly one stream, chosen so
//! that the stream's symbols share a distribution:
//!
//! ```text
//! opcode stream      the operator tag of every node, in node order
//! arity stream       P(arity | op) — the only variable arity, `Template` slots
//! edge stream        every node reference, as a *signed distance* from the node
//!                    that makes it, so a recent target is a cheap small integer
//! parameter stream   P(parameter class | op) — the `RefTarget` kind
//! length stream      the remaining integer parameters (counts, offsets, literal
//!                    lengths and ids, residual ids, spans)
//! literal stream     the shared text itself
//! ```
//!
//! The first five streams are integer sequences; their *raw* form is the
//! concatenated unsigned LEB128 of their symbols. The literal stream is raw bytes.
//! Reducing every stream to a byte string means one codec competition serves all
//! six: a stream is a byte string, and the codecs do not care what it means.
//!
//! **Why distances.** A node reference is an index into `Program::nodes`. Coding
//! the absolute index wastes symbols on programs whose references are local
//! (`Concat` children are almost always the two nodes just emitted). Coding the
//! *signed* distance `current - target`, zigzagged, makes a recent target a small
//! integer and leaves forward references representable without an escape.
//!
//! **Why the parameter-class stream is separate from the length stream.** A
//! `Ref`'s kind selects a *sub-behaviour* (a node edge versus an external id),
//! and the two have different statistics; the kind is a class, the id is a
//! magnitude. Splitting them lets each stream's model be chosen on its own.
//!
//! ## The model competition (§14.28)
//!
//! For each stream the smallest *complete* size wins among:
//!
//! ```text
//! raw             the stream bytes as they are
//! rANS            a persisted per-stream histogram plus rANS symbols
//! order-0 range   an adaptive byte model with no persisted table
//! order-1 range   the same, conditioned on the previous byte
//! ```
//!
//! Every persisted table byte is part of the candidate's payload, so a model that
//! cannot pay for its own description loses to `raw` and does not exist. The
//! adaptive orders carry no persisted table at all — their cost is only the coded
//! payload — which is why they win on small streams and rANS wins on large skewed
//! ones. `raw` is always a candidate, so no stream is ever made larger by being
//! modelled.
//!
//! ## Honesty and bounds
//!
//! [`encode_program`] is exact: [`decode_program`] is its inverse on every valid
//! program, and [`compare_program`] asserts that. Coding can still lose to raw
//! serialization on tiny programs, where the container and stream descriptors cost
//! more than the varints they replace; [`compare_program`] reports that rather than
//! hiding it, and [`best_program_bytes`] returns the smaller of the two.
//!
//! Decoding enforces the same [`Bounds`] the VM does — depth, nodes, output, work,
//! references — and returns `None` on a violation instead of allocating or looping
//! without limit. Every count read from the wire is first checked against the bytes
//! that remain, mirroring [`super::serialize`]'s earned-capacity discipline.

use super::program::{Node, Op, Program};
use super::serialize::serialize;
use super::types::{LiteralId, NodeId, RefTarget, ResidualId, Span};
use super::Bounds;
use crate::entropy::rans;
use crate::entropy::{RangeDecoder, RangeEncoder, PROB_SCALE};

/// Wire version of the coded container. Bumping it is a costed interface change.
const CODEC_VERSION: u8 = 1;

/// Number of streams; also the width of the presence mask.
const STREAM_COUNT: usize = 6;

/// The mask byte has this many meaningful bits.
const STREAM_MASK_BITS: u8 = (1 << STREAM_COUNT) - 1;

/// Longest legal unsigned LEB128 encoding of a `u64` (`ceil(64/7)`).
const MAX_VARINT_BYTES: usize = 10;

/// Largest `Template` slot list a decode will build. A forged arity must not turn
/// into an unbounded loop; no search-built skeleton approaches this.
const MAX_ARITY: usize = 1 << 20;

/// Hard per-stream ceiling on decoded *raw* bytes, independent of [`Bounds`].
/// rANS can expand a short payload into a long run, so the raw length is not
/// bounded by the input and needs its own ceiling.
const MAX_STREAM_RAW_BYTES: usize = 1 << 26; // 64 MiB

/// Hard ceiling on the decoded raw bytes of all streams together.
const MAX_TOTAL_RAW_BYTES: usize = 1 << 27; // 128 MiB

/// Adaptation rate of the adaptive byte models: `p += (target - p) >> SHIFT`.
/// Four is the smallest shift that is still fast enough to track a real stream and
/// slow enough not to chase noise.
const ADAPT_SHIFT: u32 = 4;

// Operator tags. These are the coded alphabet's meaning, not the serialized tags
// of `super::serialize`; the two are kept separate so either format can evolve.
const OP_LITERAL: u8 = 0;
const OP_CONCAT: u8 = 1;
const OP_REPEAT: u8 = 2;
const OP_REF: u8 = 3;
const OP_SLICE: u8 = 4;
const OP_TEMPLATE: u8 = 5;
const OP_PATCH: u8 = 6;

// `RefTarget` kinds, the parameter-class alphabet.
const RK_NODE: u64 = 0;
const RK_MATERIAL: u64 = 1;
const RK_SLOT: u64 = 2;

/// The six streams the plan names, in the order their mask bits are assigned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    /// One symbol per node: the operator tag.
    Opcode,
    /// One symbol per `Template` node: its slot count.
    Arity,
    /// One symbol per `Ref` node: the `RefTarget` kind.
    Parameter,
    /// One symbol per node reference: the signed, zigzagged node distance.
    Edge,
    /// Integer parameters: literal lengths/ids, counts, offsets, residual ids, spans.
    Length,
    /// The literal pool's bytes.
    Literal,
}

impl StreamKind {
    /// All streams, in mask order.
    pub const ALL: [StreamKind; STREAM_COUNT] = [
        StreamKind::Opcode,
        StreamKind::Arity,
        StreamKind::Parameter,
        StreamKind::Edge,
        StreamKind::Length,
        StreamKind::Literal,
    ];

    /// Position in [`StreamKind::ALL`], which is also the mask bit.
    pub fn index(self) -> usize {
        match self {
            StreamKind::Opcode => 0,
            StreamKind::Arity => 1,
            StreamKind::Parameter => 2,
            StreamKind::Edge => 3,
            StreamKind::Length => 4,
            StreamKind::Literal => 5,
        }
    }

    /// A stable name for receipts.
    pub fn name(self) -> &'static str {
        match self {
            StreamKind::Opcode => "opcode",
            StreamKind::Arity => "arity",
            StreamKind::Parameter => "parameter",
            StreamKind::Edge => "edge",
            StreamKind::Length => "length",
            StreamKind::Literal => "literal",
        }
    }
}

/// The candidate representations for one stream, in tie-break order (earliest
/// wins a tie, so the simpler mechanism is preferred when it costs nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamCodec {
    /// The stream bytes as they are: the incumbent, always available.
    Raw,
    /// A persisted per-stream histogram plus a byte-interleaved rANS stream.
    Rans,
    /// An adaptive byte model with no persisted table.
    Order0,
    /// The adaptive byte model conditioned on the previous byte.
    Order1,
}

impl StreamCodec {
    /// All codecs, in tie-break order.
    pub const ALL: [StreamCodec; 4] = [
        StreamCodec::Raw,
        StreamCodec::Rans,
        StreamCodec::Order0,
        StreamCodec::Order1,
    ];

    /// Position in [`StreamCodec::ALL`], for the size table.
    pub fn index(self) -> usize {
        match self {
            StreamCodec::Raw => 0,
            StreamCodec::Rans => 1,
            StreamCodec::Order0 => 2,
            StreamCodec::Order1 => 3,
        }
    }

    /// A stable name for receipts.
    pub fn name(self) -> &'static str {
        match self {
            StreamCodec::Raw => "raw",
            StreamCodec::Rans => "rans",
            StreamCodec::Order0 => "order0",
            StreamCodec::Order1 => "order1",
        }
    }

    fn id(self) -> u8 {
        self.index() as u8
    }

    fn from_id(b: u8) -> Option<Self> {
        match b {
            0 => Some(StreamCodec::Raw),
            1 => Some(StreamCodec::Rans),
            2 => Some(StreamCodec::Order0),
            3 => Some(StreamCodec::Order1),
            _ => None,
        }
    }
}

/// The winning codec for one stream, with every candidate's complete size so a
/// caller can see *why* it won — the same discipline as `state::Choice`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamChoice {
    /// The codec whose payload is `payload`.
    pub codec: StreamCodec,
    /// The winner's coded bytes. For `Rans` this includes the persisted histogram;
    /// for the adaptive orders it is only the coded payload.
    pub payload: Vec<u8>,
    /// The stream's raw byte length, which a decoder must reproduce exactly.
    pub raw_bytes: usize,
    /// Bytes of the rANS candidate that are its histogram (`0` if rANS was not a
    /// candidate). Reported even when rANS loses, so the model charge is visible.
    pub model_bytes: usize,
    /// Complete size every candidate achieved, or `usize::MAX` if it could not
    /// represent the stream (rANS on a pathological histogram).
    pub sizes: [(StreamCodec, usize); 4],
}

impl StreamChoice {
    /// The winner's complete size in bytes.
    pub fn coded_bytes(&self) -> usize {
        self.payload.len()
    }

    /// The size a candidate achieved, or `usize::MAX` if it was unavailable.
    pub fn size(&self, codec: StreamCodec) -> usize {
        self.sizes[codec.index()].1
    }

    /// A one-line receipt. Integer-only: no floats reach a receipt.
    pub fn explain(&self) -> String {
        let mut s = format!("{} wins at {} B", self.codec.name(), self.payload.len());
        for &(codec, size) in &self.sizes {
            if codec == self.codec {
                continue;
            }
            if size == usize::MAX {
                s.push_str(&format!("; {} n/a", codec.name()));
            } else {
                s.push_str(&format!("; {} {} B", codec.name(), size));
            }
        }
        if self.model_bytes > 0 {
            s.push_str(&format!(" (rans model {} B charged)", self.model_bytes));
        }
        s
    }
}

/// Per-stream cost in a whole-program breakdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamCost {
    pub kind: StreamKind,
    pub codec: StreamCodec,
    pub raw_bytes: usize,
    pub model_bytes: usize,
    pub coded_bytes: usize,
    pub candidates: [(StreamCodec, usize); 4],
}

/// A whole-program cost breakdown: which stream chose which codec, and at what size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramCost {
    /// Container header plus every stream descriptor.
    pub header_bytes: usize,
    /// One entry per stream that was present.
    pub streams: Vec<StreamCost>,
    /// Total coded size: `header_bytes + Σ coded_bytes`.
    pub total: usize,
}

impl ProgramCost {
    /// The stream cost for `kind`, if that stream was present.
    pub fn stream(&self, kind: StreamKind) -> Option<&StreamCost> {
        self.streams.iter().find(|s| s.kind == kind)
    }

    /// A multi-line receipt: one line per stream plus the header.
    pub fn explain(&self) -> String {
        let mut s = format!("{} B total ({} B header)", self.total, self.header_bytes);
        for st in &self.streams {
            s.push_str(&format!(
                "\n  {:9} {:7} raw {:6} -> coded {:6} (rans model {} B)",
                st.kind.name(),
                st.codec.name(),
                st.raw_bytes,
                st.coded_bytes,
                st.model_bytes
            ));
        }
        s
    }
}

/// The comparison the plan asks for: raw serialization against the coded form.
///
/// The `decoded_matches` flag is the exactness assertion made at report time, and
/// `wins` is honest about losing: a tiny program can code larger than it
/// serializes, and this struct says so instead of hiding it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    /// `serialize(program).len()`.
    pub raw: usize,
    /// `encode_program(program).len()`.
    pub coded: usize,
    /// The smaller of the two — what [`best_program_bytes`] returns.
    pub best: usize,
    /// Whether `decode_program(encode_program(p)) == Some(p)`.
    pub decoded_matches: bool,
    /// Coded / raw in per-mille (1000 = parity), integer-only.
    pub ratio_per_mille: u32,
    /// The per-stream breakdown.
    pub cost: ProgramCost,
}

impl Comparison {
    /// Whether the coded form is strictly smaller than raw serialization.
    pub fn wins(&self) -> bool {
        self.coded < self.raw
    }

    /// A receipt naming the ratio and the winner.
    pub fn explain(&self) -> String {
        format!(
            "raw {} B, coded {} B ({}‰, {}), best {} B, exact {}",
            self.raw,
            self.coded,
            self.ratio_per_mille,
            if self.wins() { "coding wins" } else { "raw wins" },
            self.best,
            self.decoded_matches
        )
    }
}

// ---------------------------------------------------------------------------
// Varints. A local copy of the serializer's primitives: the coded container is
// its own format, so it owns its own parser, and a decoder must never trust a
// length it has not bounded.
// ---------------------------------------------------------------------------

/// A capacity a decoded count has *earned*: each item costs at least `min_bytes`,
/// so a forged count cannot amplify a small input into a large allocation.
fn earned_capacity(count: usize, remaining: usize, min_bytes: usize) -> usize {
    count.min(remaining / min_bytes + 1)
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

/// Read one unsigned LEB128 value, rejecting truncation, over-long runs and an
/// encoding whose bits do not fit a `u64` rather than wrapping.
fn get_uvarint(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..MAX_VARINT_BYTES {
        let byte = *b.get(*pos)?;
        *pos += 1;
        if shift >= 64 {
            return None;
        }
        let low = (byte & 0x7f) as u64;
        if shift == 63 && low > 1 {
            return None;
        }
        v |= low << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
    None
}

/// Zigzag a signed value to an unsigned one, so small magnitudes of either sign
/// are small varints. Forward node references are the only negative distances.
fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

fn unzigzag(z: u64) -> i64 {
    ((z >> 1) as i64) ^ -((z & 1) as i64)
}

fn as_u32(v: u64) -> Option<u32> {
    if v <= u32::MAX as u64 {
        Some(v as u32)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// The codec competition for one byte stream.
// ---------------------------------------------------------------------------

/// Whether `entropy::rans::Table::from_counts` can normalise these counts without
/// tripping its own assertion. That function is the single source of truth, but it
/// *panics* on a pathological vector, and a forged archive must produce `None`, not
/// a panic — so the dangerous adjustment is mirrored here and checked first.
/// (The same discipline as `super::state::rans_counts_ok`.)
fn rans_counts_ok(counts: &[u32]) -> bool {
    let m: u64 = 1u64 << rans::SCALE_BITS;
    if counts.is_empty() || counts.len() as u64 > m {
        return false;
    }
    let total: u64 = counts.iter().map(|&c| c as u64).sum();
    if total == 0 {
        return true; // the degenerate uniform branch, which has no assertion
    }
    let mut assigned: u64 = 0;
    let mut max_freq: u64 = 0;
    for &c in counts {
        if c == 0 {
            continue;
        }
        let f = ((c as u64 * m) / total).max(1);
        assigned += f;
        if f > max_freq {
            max_freq = f;
        }
    }
    if assigned == m {
        return true;
    }
    max_freq as i64 + (m as i64 - assigned as i64) >= 1
}

fn rans_table(counts: &[u32]) -> Option<rans::Table> {
    if !rans_counts_ok(counts) {
        return None;
    }
    Some(rans::Table::from_counts(counts))
}

/// Code `raw` with a histogram over its own bytes. The histogram is written in
/// front of the symbols and charged to them, because a model that cannot pay for
/// its own persisted bytes does not exist. Returns `(payload, model_len)`.
fn rans_encode_bytes(raw: &[u8]) -> Option<(Vec<u8>, usize)> {
    if raw.is_empty() {
        return None;
    }
    let alphabet = (*raw.iter().max()? as usize) + 1;
    if alphabet == 0 || alphabet > (1usize << rans::SCALE_BITS) {
        return None;
    }
    let mut counts = vec![0u32; alphabet];
    for &b in raw {
        counts[b as usize] = counts[b as usize].checked_add(1)?;
    }
    let table = rans_table(&counts)?;
    let mut out = Vec::new();
    put_uvarint(&mut out, alphabet as u64);
    for &c in &counts {
        put_uvarint(&mut out, c as u64);
    }
    let model_len = out.len();
    let symbols: Vec<u16> = raw.iter().map(|&b| b as u16).collect();
    out.extend_from_slice(&rans::encode(&symbols, &table));
    Some((out, model_len))
}

/// Decode a rANS payload back to exactly `raw_len` bytes.
fn rans_decode_bytes(payload: &[u8], raw_len: usize) -> Option<Vec<u8>> {
    let mut pos = 0usize;
    let alphabet = get_uvarint(payload, &mut pos)?;
    if alphabet == 0 || alphabet > 256 {
        return None;
    }
    let alphabet = alphabet as usize;
    let mut counts = vec![0u32; alphabet];
    for c in counts.iter_mut() {
        *c = as_u32(get_uvarint(payload, &mut pos)?)?;
    }
    let table = rans_table(&counts)?;
    let mut dec = rans::Decoder::new(&payload[pos..])?;
    let mut out: Vec<u16> = Vec::new();
    if !dec.decode_n(&table, raw_len, &mut out) {
        return None;
    }
    Some(out.into_iter().map(|s| s as u8).collect())
}

/// An adaptive byte model: a 256-leaf binary tree of bit probabilities, optionally
/// one tree per previous byte. No table is persisted, so its only cost is the coded
/// payload — which is why it is the candidate that can win on small streams.
struct ByteModel {
    /// `probs[ctx * 256 + node]`, `node` in `1..=255`; context 0 when order-0.
    probs: Vec<u16>,
    order1: bool,
}

impl ByteModel {
    fn new(order1: bool) -> Self {
        let n = if order1 { 256 * 256 } else { 256 };
        ByteModel {
            probs: vec![32768u16; n],
            order1,
        }
    }

    #[inline]
    fn base(&self, prev: usize) -> usize {
        if self.order1 {
            prev << 8
        } else {
            0
        }
    }
}

fn adaptive_encode(raw: &[u8], order1: bool) -> Vec<u8> {
    let mut model = ByteModel::new(order1);
    let mut enc = RangeEncoder::new();
    let mut prev = 0usize;
    for &b in raw {
        let base = model.base(prev);
        let mut node = 1usize;
        for k in (0..8).rev() {
            let bit = ((b >> k) & 1) as u32;
            let idx = base + node;
            let p = (model.probs[idx] as u32 >> 4).clamp(1, PROB_SCALE - 1);
            enc.encode(bit, p);
            let target: i32 = if bit != 0 { 65535 } else { 0 };
            let cur = model.probs[idx] as i32;
            model.probs[idx] = (cur + ((target - cur) >> ADAPT_SHIFT)) as u16;
            node = (node << 1) | bit as usize;
        }
        prev = b as usize;
    }
    enc.finish()
}

fn adaptive_decode(payload: &[u8], raw_len: usize, order1: bool) -> Option<Vec<u8>> {
    let mut model = ByteModel::new(order1);
    let mut dec = RangeDecoder::new(payload);
    let mut out = Vec::with_capacity(raw_len);
    let mut prev = 0usize;
    for _ in 0..raw_len {
        let base = model.base(prev);
        let mut node = 1usize;
        let mut sym: u8 = 0;
        for _ in 0..8 {
            let idx = base + node;
            let p = (model.probs[idx] as u32 >> 4).clamp(1, PROB_SCALE - 1);
            let bit = dec.decode(p);
            let target: i32 = if bit != 0 { 65535 } else { 0 };
            let cur = model.probs[idx] as i32;
            model.probs[idx] = (cur + ((target - cur) >> ADAPT_SHIFT)) as u16;
            node = (node << 1) | bit as usize;
            sym = (sym << 1) | bit as u8;
        }
        out.push(sym);
        prev = sym as usize;
    }
    Some(out)
}

/// Decode a stream payload back to its raw bytes, of exactly `raw_len` bytes.
fn decode_payload(
    codec: StreamCodec,
    payload: &[u8],
    raw_len: usize,
    cap: usize,
) -> Option<Vec<u8>> {
    // The cap is checked before any allocation: a forged raw length cannot make
    // the decoder reserve or loop without limit.
    if raw_len > cap {
        return None;
    }
    match codec {
        StreamCodec::Raw => {
            if payload.len() != raw_len {
                return None;
            }
            Some(payload.to_vec())
        }
        StreamCodec::Rans => rans_decode_bytes(payload, raw_len),
        StreamCodec::Order0 => adaptive_decode(payload, raw_len, false),
        StreamCodec::Order1 => adaptive_decode(payload, raw_len, true),
    }
}

/// Run every candidate on `raw` and return the smallest complete result.
///
/// This is deliberately public: it is the per-stream measurement a caller needs to
/// see *why* a stream was coded the way it was, and it is the function whose
/// honesty the model-charge test pins.
pub fn choose_stream(raw: &[u8]) -> StreamChoice {
    let mut sizes = StreamCodec::ALL.map(|c| (c, usize::MAX));
    let mut best_codec = StreamCodec::Raw;
    let mut best_payload = raw.to_vec();
    sizes[StreamCodec::Raw.index()] = (StreamCodec::Raw, raw.len());

    let mut model_bytes = 0usize;
    if !raw.is_empty() {
        if let Some((payload, model)) = rans_encode_bytes(raw) {
            sizes[StreamCodec::Rans.index()] = (StreamCodec::Rans, payload.len());
            model_bytes = model;
            // Strict `<` keeps the earlier (simpler) codec on a tie, so a table is
            // never chosen when it bought nothing.
            if payload.len() < best_payload.len() {
                best_codec = StreamCodec::Rans;
                best_payload = payload;
            }
        }
        let p0 = adaptive_encode(raw, false);
        sizes[StreamCodec::Order0.index()] = (StreamCodec::Order0, p0.len());
        if p0.len() < best_payload.len() {
            best_codec = StreamCodec::Order0;
            best_payload = p0;
        }
        let p1 = adaptive_encode(raw, true);
        sizes[StreamCodec::Order1.index()] = (StreamCodec::Order1, p1.len());
        if p1.len() < best_payload.len() {
            best_codec = StreamCodec::Order1;
            best_payload = p1;
        }
    }

    StreamChoice {
        codec: best_codec,
        payload: best_payload,
        raw_bytes: raw.len(),
        model_bytes,
        sizes,
    }
}

// ---------------------------------------------------------------------------
// Splitting a program into streams (§14.29), and the container that frames them.
// ---------------------------------------------------------------------------

fn op_tag(op: &Op) -> u8 {
    match op {
        Op::Literal(_) => OP_LITERAL,
        Op::Concat { .. } => OP_CONCAT,
        Op::Repeat { .. } => OP_REPEAT,
        Op::Ref { .. } => OP_REF,
        Op::Slice { .. } => OP_SLICE,
        Op::Template { .. } => OP_TEMPLATE,
        Op::Patch { .. } => OP_PATCH,
    }
}

/// The signed distance from node `at` to `target`, zigzagged.
fn edge_distance(at: usize, target: u32) -> u64 {
    zigzag(at as i64 - target as i64)
}

/// Visit the node-valued children of `node` in the canonical edge order. This is
/// the single definition of edge order: encode and decode both use it, so a
/// mismatch is impossible by construction.
fn for_each_child(node: &Node, mut f: impl FnMut(NodeId)) {
    match &node.op {
        Op::Literal(_) => {}
        Op::Concat { parts } => {
            f(parts[0]);
            f(parts[1]);
        }
        Op::Repeat { child, .. } => f(*child),
        Op::Ref { target } => {
            if let RefTarget::Node(n) = target {
                f(*n);
            }
        }
        Op::Slice { src, .. } => f(*src),
        Op::Template { program, slots } => {
            f(*program);
            for s in slots {
                f(*s);
            }
        }
        Op::Patch { base, .. } => f(*base),
    }
}

/// Split `p` into the six streams. A stream with no bytes is `None` and costs
/// nothing in the container.
fn split_streams(p: &Program) -> [Option<Vec<u8>>; STREAM_COUNT] {
    let mut opcode = Vec::new();
    let mut arity = Vec::new();
    let mut parameter = Vec::new();
    let mut edge = Vec::new();
    let mut length = Vec::new();
    let mut literal: Vec<u8> = Vec::new();

    // Literal lengths and bytes lead the length and literal streams.
    for lit in &p.literals {
        put_uvarint(&mut length, lit.len() as u64);
        literal.extend_from_slice(lit);
    }

    for (i, node) in p.nodes.iter().enumerate() {
        put_uvarint(&mut opcode, op_tag(&node.op) as u64);
        match &node.op {
            Op::Literal(id) => put_uvarint(&mut length, id.0 as u64),
            Op::Concat { parts } => {
                put_uvarint(&mut edge, edge_distance(i, parts[0].0));
                put_uvarint(&mut edge, edge_distance(i, parts[1].0));
            }
            Op::Repeat { child, count } => {
                put_uvarint(&mut edge, edge_distance(i, child.0));
                put_uvarint(&mut length, *count as u64);
            }
            Op::Ref { target } => match target {
                RefTarget::Node(n) => {
                    put_uvarint(&mut parameter, RK_NODE);
                    put_uvarint(&mut edge, edge_distance(i, n.0));
                }
                RefTarget::Material(m) => {
                    put_uvarint(&mut parameter, RK_MATERIAL);
                    put_uvarint(&mut length, *m as u64);
                }
                RefTarget::Slot(s) => {
                    put_uvarint(&mut parameter, RK_SLOT);
                    put_uvarint(&mut length, *s as u64);
                }
            },
            Op::Slice { src, from, len } => {
                put_uvarint(&mut edge, edge_distance(i, src.0));
                put_uvarint(&mut length, *from as u64);
                put_uvarint(&mut length, *len as u64);
            }
            Op::Template { program, slots } => {
                put_uvarint(&mut edge, edge_distance(i, program.0));
                put_uvarint(&mut arity, slots.len() as u64);
                for s in slots {
                    put_uvarint(&mut edge, edge_distance(i, s.0));
                }
            }
            Op::Patch { base, residual } => {
                put_uvarint(&mut edge, edge_distance(i, base.0));
                put_uvarint(&mut length, residual.0 as u64);
            }
        }
        // Span is search metadata, but it is charged honestly: it rides the length
        // stream rather than being dropped, so a search that cannot pay for its own
        // bookkeeping is visible.
        put_uvarint(&mut length, node.span.from);
        put_uvarint(&mut length, node.span.len);
    }

    let streams = [
        opcode, arity, parameter, edge, length, literal,
    ];
    streams.map(|s| if s.is_empty() { None } else { Some(s) })
}

/// Frame already-chosen streams into the container. Kept separate from the
/// choice so a test can assemble a deliberately malformed container.
fn assemble(
    node_count: usize,
    root: u32,
    lit_count: usize,
    chosen: &[Option<StreamChoice>; STREAM_COUNT],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(CODEC_VERSION);
    put_uvarint(&mut out, node_count as u64);
    put_uvarint(&mut out, root as u64);
    put_uvarint(&mut out, lit_count as u64);
    let mut mask = 0u8;
    for (i, c) in chosen.iter().enumerate() {
        if c.is_some() {
            mask |= 1 << i;
        }
    }
    out.push(mask);
    for c in chosen.iter().flatten() {
        out.push(c.codec.id());
        put_uvarint(&mut out, c.payload.len() as u64);
        put_uvarint(&mut out, c.raw_bytes as u64);
        out.extend_from_slice(&c.payload);
    }
    out
}

/// Encode `p` and return both the bytes and the per-stream cost breakdown.
///
/// The header is the container and descriptors; `total` is `bytes.len()`.
pub fn encode_program_with_cost(p: &Program) -> (Vec<u8>, ProgramCost) {
    let raws = split_streams(p);
    let mut chosen: [Option<StreamChoice>; STREAM_COUNT] = std::array::from_fn(|_| None);
    let mut costs: Vec<StreamCost> = Vec::new();
    for (i, raw) in raws.iter().enumerate() {
        if let Some(raw) = raw {
            let c = choose_stream(raw);
            costs.push(StreamCost {
                kind: StreamKind::ALL[i],
                codec: c.codec,
                raw_bytes: c.raw_bytes,
                model_bytes: c.model_bytes,
                coded_bytes: c.payload.len(),
                candidates: c.sizes,
            });
            chosen[i] = Some(c);
        }
    }
    let bytes = assemble(p.nodes.len(), p.root.0, p.literals.len(), &chosen);
    let payload_total: usize = costs.iter().map(|c| c.coded_bytes).sum();
    let header_bytes = bytes.len() - payload_total;
    let total = bytes.len();
    (
        bytes,
        ProgramCost {
            header_bytes,
            streams: costs,
            total,
        },
    )
}

/// Encode `p` into its coded container. Exact inverse of [`decode_program`].
pub fn encode_program(p: &Program) -> Vec<u8> {
    encode_program_with_cost(p).0
}

// ---------------------------------------------------------------------------
// Decoding, with the five bounds enforced.
// ---------------------------------------------------------------------------

/// Enforce the same five [`Bounds`] the VM does, for the part of each that a
/// program's *structure* determines.
///
/// A decode cannot know the runtime output length (a `Repeat` amplifies at
/// execute time), so it enforces a conservative, never-false lower bound on work
/// and rejects anything the VM would reject structurally: too many nodes, a
/// nesting deeper than `max_depth`, a reference cycle, an over-large reachable
/// literal, too many external material references, and a work lower bound. The
/// runtime-only remainder stays enforced by [`super::execute`].
fn enforce_bounds(p: &Program, bounds: &Bounds) -> Option<()> {
    let n = p.nodes.len();
    if p.root.0 as usize >= n {
        return None;
    }
    if n as u64 > bounds.max_nodes as u64 {
        return None;
    }

    enum Frame {
        Enter(usize),
        Exit(usize),
    }
    let mut color = vec![0u8; n]; // 0 white, 1 on stack, 2 done
    let mut depth = vec![0u32; n];
    let mut stack = vec![Frame::Enter(p.root.0 as usize)];
    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Enter(u) => {
                match color[u] {
                    1 => return None, // a back edge: the VM's RefCycle
                    2 => continue,
                    _ => {}
                }
                color[u] = 1;
                stack.push(Frame::Exit(u));
                let mut cycle = false;
                for_each_child(&p.nodes[u], |c| {
                    if color[c.0 as usize] == 1 {
                        cycle = true;
                    }
                });
                if cycle {
                    return None;
                }
                for_each_child(&p.nodes[u], |c| {
                    if color[c.0 as usize] == 0 {
                        stack.push(Frame::Enter(c.0 as usize));
                    }
                });
            }
            Frame::Exit(u) => {
                color[u] = 2;
                let mut d = 0u32;
                for_each_child(&p.nodes[u], |c| {
                    let cd = depth[c.0 as usize].saturating_add(1);
                    if cd > d {
                        d = cd;
                    }
                });
                depth[u] = d;
                if d > bounds.max_depth as u32 {
                    return None;
                }
            }
        }
    }

    // Reachable-only checks, so a program is never rejected for material it does
    // not use.
    let mut material_refs: u64 = 0;
    let mut work_floor: u64 = 0;
    for u in 0..n {
        if color[u] != 2 {
            continue;
        }
        work_floor = work_floor.checked_add(1)?; // one evaluation charge per node
        match &p.nodes[u].op {
            Op::Literal(id) => {
                let len = p.literals.get(id.0 as usize)?.len() as u64;
                if len > bounds.max_output {
                    return None;
                }
                work_floor = work_floor.checked_add(len)?;
            }
            Op::Ref {
                target: RefTarget::Material(_),
            } => {
                material_refs = material_refs.checked_add(1)?;
                if material_refs > bounds.max_refs as u64 {
                    return None;
                }
            }
            _ => {}
        }
    }
    if work_floor > bounds.max_work {
        return None;
    }
    Some(())
}

/// Decode a coded program under the default [`Bounds`].
pub fn decode_program(b: &[u8]) -> Option<Program> {
    decode_program_with(b, &Bounds::default())
}

/// Decode a coded program under explicit bounds. `None` is a *typed* rejection:
/// a forged archive is refused, never trusted and never panicked on.
pub fn decode_program_with(b: &[u8], bounds: &Bounds) -> Option<Program> {
    let mut pos = 0usize;
    if *b.get(pos)? != CODEC_VERSION {
        return None;
    }
    pos += 1;

    let node_count_u = get_uvarint(b, &mut pos)?;
    let root_u = get_uvarint(b, &mut pos)?;
    let lit_count_u = get_uvarint(b, &mut pos)?;
    if node_count_u > bounds.max_nodes as u64 || lit_count_u > u32::MAX as u64 {
        return None;
    }
    if root_u > u32::MAX as u64 {
        return None;
    }
    // Every node costs at least one byte somewhere, so a count larger than the
    // remaining input is impossible and is rejected before any allocation.
    let remaining = b.len().saturating_sub(pos);
    if node_count_u > remaining as u64 || lit_count_u > remaining as u64 {
        return None;
    }
    let node_count = node_count_u as usize;
    let lit_count = lit_count_u as usize;

    let mask = *b.get(pos)?;
    pos += 1;
    if mask & !STREAM_MASK_BITS != 0 {
        return None;
    }

    let mut raws: [Option<Vec<u8>>; STREAM_COUNT] = [None, None, None, None, None, None];
    let mut total_raw = 0usize;
    for i in 0..STREAM_COUNT {
        if mask & (1 << i) == 0 {
            continue;
        }
        let codec = StreamCodec::from_id(*b.get(pos)?)?;
        pos += 1;
        let coded_len = get_uvarint(b, &mut pos)?;
        let raw_len = get_uvarint(b, &mut pos)?;
        if coded_len > (b.len() - pos) as u64 {
            return None;
        }
        let cap = MAX_STREAM_RAW_BYTES.min(bounds.max_work as usize);
        if raw_len > cap as u64 {
            return None;
        }
        let coded_len = coded_len as usize;
        let raw_len = raw_len as usize;
        total_raw = total_raw.checked_add(raw_len)?;
        if total_raw > MAX_TOTAL_RAW_BYTES {
            return None;
        }
        let payload = &b[pos..pos + coded_len];
        pos += coded_len;
        let raw = decode_payload(codec, payload, raw_len, cap)?;
        if raw.len() != raw_len {
            return None;
        }
        raws[i] = Some(raw);
    }
    if pos != b.len() {
        return None;
    }

    let empty: &[u8] = &[];
    let opcode_raw = raws[0].as_deref().unwrap_or(empty);

    // Opcodes. Every other stream's shape is read through this one.
    let mut ops: Vec<u8> = Vec::with_capacity(earned_capacity(node_count, opcode_raw.len(), 1));
    {
        let mut p = 0usize;
        for _ in 0..node_count {
            let t = get_uvarint(opcode_raw, &mut p)?;
            if t > OP_PATCH as u64 {
                return None;
            }
            ops.push(t as u8);
        }
        if p != opcode_raw.len() {
            return None;
        }
    }

    // Arity: only `Template` has a variable arity, so only those nodes carry a
    // symbol. A stream present but unconsumed is a rejection, not slack.
    let mut slot_counts = vec![0u32; node_count];
    {
        let ar = raws[StreamKind::Arity.index()].as_deref().unwrap_or(empty);
        let mut p = 0usize;
        for i in 0..node_count {
            if ops[i] == OP_TEMPLATE {
                let c = get_uvarint(ar, &mut p)?;
                if c > MAX_ARITY as u64 {
                    return None;
                }
                slot_counts[i] = c as u32;
            }
        }
        if p != ar.len() {
            return None;
        }
    }

    // Parameter class: the `RefTarget` kind of every `Ref` node.
    let mut kinds = vec![0u8; node_count];
    {
        let pr = raws[StreamKind::Parameter.index()].as_deref().unwrap_or(empty);
        let mut p = 0usize;
        for i in 0..node_count {
            if ops[i] == OP_REF {
                let k = get_uvarint(pr, &mut p)?;
                if k > RK_SLOT {
                    return None;
                }
                kinds[i] = k as u8;
            }
        }
        if p != pr.len() {
            return None;
        }
    }

    // Edges: resolved to node ids in canonical order, rejecting any target outside
    // the program — the decode-time half of the VM's BadRef.
    let mut edges: Vec<Vec<u32>> = Vec::with_capacity(earned_capacity(node_count, remaining, 1));
    {
        let er = raws[StreamKind::Edge.index()].as_deref().unwrap_or(empty);
        let mut p = 0usize;
        for i in 0..node_count {
            let count = match ops[i] {
                OP_CONCAT => 2,
                OP_REPEAT | OP_SLICE | OP_PATCH => 1,
                OP_TEMPLATE => 1usize.checked_add(slot_counts[i] as usize)?,
                OP_REF => {
                    if kinds[i] == RK_NODE as u8 {
                        1
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            let mut list = Vec::with_capacity(count.min(8));
            for _ in 0..count {
                let z = get_uvarint(er, &mut p)?;
                let target = i as i64 - unzigzag(z);
                if target < 0 || target >= node_count as i64 {
                    return None;
                }
                list.push(target as u32);
            }
            edges.push(list);
        }
        if p != er.len() {
            return None;
        }
    }

    // Lengths and, from them, the nodes themselves. Literal lengths lead.
    let len_raw = raws[StreamKind::Length.index()].as_deref().unwrap_or(empty);
    let mut lit_lens: Vec<usize> = Vec::with_capacity(earned_capacity(lit_count, len_raw.len(), 1));
    let mut lp = 0usize;
    for _ in 0..lit_count {
        let l = get_uvarint(len_raw, &mut lp)?;
        if l > MAX_STREAM_RAW_BYTES as u64 {
            return None;
        }
        lit_lens.push(l as usize);
    }

    let mut nodes: Vec<Node> = Vec::with_capacity(earned_capacity(node_count, len_raw.len(), 1));
    for i in 0..node_count {
        let e = &edges[i];
        let op = match ops[i] {
            OP_LITERAL => {
                let id = as_u32(get_uvarint(len_raw, &mut lp)?)?;
                if id as usize >= lit_count {
                    return None;
                }
                Op::Literal(LiteralId(id))
            }
            OP_CONCAT => Op::Concat {
                parts: [NodeId(e[0]), NodeId(e[1])],
            },
            OP_REPEAT => {
                let child = NodeId(e[0]);
                let count = as_u32(get_uvarint(len_raw, &mut lp)?)?;
                Op::Repeat { child, count }
            }
            OP_REF => match kinds[i] {
                k if k == RK_NODE as u8 => Op::Ref {
                    target: RefTarget::Node(NodeId(e[0])),
                },
                k if k == RK_MATERIAL as u8 => Op::Ref {
                    target: RefTarget::Material(as_u32(get_uvarint(len_raw, &mut lp)?)?),
                },
                _ => Op::Ref {
                    target: RefTarget::Slot(as_u32(get_uvarint(len_raw, &mut lp)?)?),
                },
            },
            OP_SLICE => {
                let src = NodeId(e[0]);
                let from = as_u32(get_uvarint(len_raw, &mut lp)?)?;
                let len = as_u32(get_uvarint(len_raw, &mut lp)?)?;
                Op::Slice { src, from, len }
            }
            OP_TEMPLATE => {
                let program = NodeId(e[0]);
                let slots: Vec<NodeId> = e[1..].iter().map(|&x| NodeId(x)).collect();
                Op::Template { program, slots }
            }
            OP_PATCH => {
                let base = NodeId(e[0]);
                let residual = ResidualId(as_u32(get_uvarint(len_raw, &mut lp)?)?);
                Op::Patch { base, residual }
            }
            _ => return None,
        };
        let from = get_uvarint(len_raw, &mut lp)?;
        let len = get_uvarint(len_raw, &mut lp)?;
        nodes.push(Node {
            op,
            span: Span { from, len },
        });
    }
    if lp != len_raw.len() {
        return None;
    }

    // Literal bytes, carved by the lengths read above.
    let literal_raw = raws[StreamKind::Literal.index()].as_deref().unwrap_or(empty);
    let mut literals: Vec<Vec<u8>> = Vec::with_capacity(earned_capacity(lit_count, literal_raw.len(), 1));
    let mut q = 0usize;
    for &l in &lit_lens {
        if l > literal_raw.len() - q {
            return None;
        }
        literals.push(literal_raw[q..q + l].to_vec());
        q += l;
    }
    if q != literal_raw.len() {
        return None;
    }

    let program = Program {
        nodes,
        literals,
        root: NodeId(root_u as u32),
    };
    if !program.ids_valid() {
        return None;
    }
    enforce_bounds(&program, bounds)?;
    Some(program)
}

// ---------------------------------------------------------------------------
// The comparison that matters.
// ---------------------------------------------------------------------------

/// The smaller of raw serialization and the coded form, so a caller that may
/// choose its representation is never forced to pay more than the incumbent.
pub fn best_program_bytes(p: &Program) -> usize {
    serialize(p).len().min(encode_program(p).len())
}

/// Report raw versus coded for `p`, with a decoded-equality assertion and the
/// per-stream breakdown.
///
/// The assertion is checked only for a structurally valid program (`ids_valid`);
/// a program with dangling ids is not something `decode_program` promises to
/// recover, and reporting on it should not panic.
pub fn compare_program(p: &Program) -> Comparison {
    let raw = serialize(p).len();
    let (coded_bytes, cost) = encode_program_with_cost(p);
    let coded = coded_bytes.len();
    let decoded_matches = decode_program(&coded_bytes).as_ref() == Some(p);
    if p.ids_valid() {
        assert!(decoded_matches, "progcodec encode/decode is not exact");
    }
    let ratio_per_mille = if raw == 0 {
        1000
    } else {
        (coded as u64 * 1000 / raw as u64) as u32
    };
    Comparison {
        raw,
        coded,
        best: raw.min(coded),
        decoded_matches,
        ratio_per_mille,
        cost,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(op: Op) -> Node {
        Node {
            op,
            span: Span::EMPTY,
        }
    }

    fn lit(i: u32) -> Node {
        n(Op::Literal(LiteralId(i)))
    }

    /// The literal-only program: the smallest program there is.
    fn literal_only() -> Program {
        Program {
            nodes: vec![lit(0)],
            literals: vec![b"hello world".to_vec()],
            root: NodeId(0),
        }
    }

    /// A left-deep `Concat` tree whose longest root path is exactly `max_depth`
    /// edges, so it round-trips under the default bounds while still being deep.
    fn deep_concat(leaves: u32) -> Program {
        assert!(leaves >= 2);
        // Every leaf shares literal 0, so the program's ids are all valid.
        let mut nodes: Vec<Node> = (0..leaves).map(|_| lit(0)).collect();
        let mut acc = NodeId(0);
        for k in 1..leaves {
            let id = NodeId(nodes.len() as u32);
            nodes.push(n(Op::Concat {
                parts: [acc, NodeId(k)],
            }));
            acc = id;
        }
        let root = acc;
        Program {
            nodes,
            literals: vec![b"x".to_vec()],
            root,
        }
    }

    fn repeat_large() -> Program {
        Program {
            nodes: vec![lit(0), n(Op::Repeat { child: NodeId(0), count: 1_000_000 })],
            literals: vec![b"ab".to_vec()],
            root: NodeId(1),
        }
    }

    fn slice_and_ref() -> Program {
        // literals are shared: node 2 slices node 1, then repeats it, and node 4
        // references node 2. On enwik-scale skeletons stages like this are common.
        Program {
            nodes: vec![
                lit(0), // 0
                lit(1), // 1
                n(Op::Slice {
                    src: NodeId(1),
                    from: 2,
                    len: 5,
                }), // 2
                n(Op::Repeat {
                    child: NodeId(2),
                    count: 3,
                }), // 3
                n(Op::Ref {
                    target: RefTarget::Node(NodeId(2)),
                }), // 4
                n(Op::Ref {
                    target: RefTarget::Material(9),
                }), // 5
                n(Op::Slice {
                    src: NodeId(3),
                    from: 0,
                    len: 1,
                }), // 6
            ],
            literals: vec![b"shared".to_vec(), b"text".to_vec()],
            root: NodeId(6),
        }
    }

    fn patch_program() -> Program {
        Program {
            nodes: vec![
                lit(0), // 0
                lit(1), // 1
                n(Op::Concat {
                    parts: [NodeId(0), NodeId(1)],
                }), // 2
                n(Op::Patch {
                    base: NodeId(2),
                    residual: ResidualId(7),
                }), // 3
            ],
            literals: vec![b"abcdef".to_vec(), b"ghijkl".to_vec()],
            root: NodeId(3),
        }
    }

    fn template_program() -> Program {
        Program {
            nodes: vec![
                lit(0), // 0
                lit(1), // 1
                n(Op::Ref {
                    target: RefTarget::Slot(0),
                }), // 2
                n(Op::Template {
                    program: NodeId(2),
                    slots: vec![NodeId(0), NodeId(1)],
                }), // 3
            ],
            literals: vec![b"{{{".to_vec(), b"}}}".to_vec()],
            root: NodeId(3),
        }
    }

    /// Many repeated opcodes and repeated literals: the case the plan expects to
    /// pay, because the opcode / edge / length streams collapse and the literal
    /// stream is one text block repeated.
    fn repeated_ops_and_literals() -> Program {
        let text = b"shared-template-text".to_vec();
        let literals: Vec<Vec<u8>> = vec![text; 40];
        let mut nodes: Vec<Node> = (0..40).map(lit).collect();
        for i in 0..40u32 {
            nodes.push(n(Op::Concat {
                parts: [NodeId(i), NodeId((i + 1) % 40)],
            }));
        }
        let root = NodeId(nodes.len() as u32 - 1);
        Program {
            nodes,
            literals,
            root,
        }
    }

    /// A realistic template-style skeleton: a handful of shared strings combined
    /// by a *balanced* `Concat` reduction, then repeated and sliced. ~50 nodes,
    /// depth well inside the default bound, so it round-trips under `decode_program`.
    fn template_style() -> Program {
        let strings: [&[u8]; 6] = [
            b"{{Infobox",
            b"|name=",
            b"|image=",
            b"|caption=",
            b"}}",
            b"[[Category:",
        ];
        let literals: Vec<Vec<u8>> = strings.iter().map(|s| s.to_vec()).collect();

        // 24 leaves over the 6 shared literals, reduced pairwise so the tree depth
        // is ceil(log2(24)) = 5 rather than 23.
        let mut nodes: Vec<Node> = Vec::new();
        let mut level: Vec<NodeId> = Vec::new();
        for i in 0..24u32 {
            let id = NodeId(nodes.len() as u32);
            nodes.push(lit(i % 6));
            level.push(id);
        }
        while level.len() > 1 {
            let mut next: Vec<NodeId> = Vec::new();
            let mut k = 0usize;
            while k + 1 < level.len() {
                let id = NodeId(nodes.len() as u32);
                nodes.push(n(Op::Concat {
                    parts: [level[k], level[k + 1]],
                }));
                next.push(id);
                k += 2;
            }
            if k < level.len() {
                next.push(level[k]);
            }
            level = next;
        }
        let body = level[0];

        nodes.push(n(Op::Repeat {
            child: body,
            count: 3,
        }));
        let rep = NodeId(nodes.len() as u32 - 1);
        nodes.push(n(Op::Slice {
            src: rep,
            from: 0,
            len: 64,
        }));
        let sliced = NodeId(nodes.len() as u32 - 1);
        nodes.push(n(Op::Ref {
            target: RefTarget::Node(sliced),
        }));
        let referenced = NodeId(nodes.len() as u32 - 1);
        nodes.push(n(Op::Patch {
            base: referenced,
            residual: ResidualId(0),
        }));
        let root = NodeId(nodes.len() as u32 - 1);
        Program {
            nodes,
            literals,
            root,
        }
    }

    fn shapes() -> Vec<Program> {
        vec![
            literal_only(),
            deep_concat(17),
            repeat_large(),
            slice_and_ref(),
            patch_program(),
            template_program(),
            repeated_ops_and_literals(),
            template_style(),
        ]
    }

    /// Assemble a container from raw stream bytes, for the malformed-input tests.
    fn container(
        node_count: usize,
        root: u32,
        lit_count: usize,
        streams: &[(StreamKind, Vec<u8>)],
    ) -> Vec<u8> {
        let mut chosen: [Option<StreamChoice>; STREAM_COUNT] = std::array::from_fn(|_| None);
        for (kind, raw) in streams {
            chosen[kind.index()] = Some(choose_stream(raw));
        }
        assemble(node_count, root, lit_count, &chosen)
    }

    #[test]
    fn round_trips_every_shape() {
        for p in shapes() {
            let root = p.root.0;
            let bytes = encode_program(&p);
            assert_eq!(decode_program(&bytes), Some(p), "{root}");
        }
    }

    #[test]
    fn round_trips_nonempty_spans() {
        // Spans are search metadata but charged, so they must survive the codec.
        let mut p = template_style();
        p.nodes[3].span = Span {
            from: 11,
            len: 4096,
        };
        p.nodes[10].span = Span {
            from: 999,
            len: 1,
        };
        assert_eq!(decode_program(&encode_program(&p)), Some(p));
    }

    #[test]
    fn coding_beats_raw_on_repeated_opcodes_and_literals() {
        let p = repeated_ops_and_literals();
        let cmp = compare_program(&p);
        assert!(
            cmp.coded < cmp.raw,
            "expected a real win: {}",
            cmp.explain()
        );
        // The win must be more than noise: a factor of two at least.
        assert!(
            cmp.coded * 2 < cmp.raw,
            "win too small: {}",
            cmp.explain()
        );
        assert!(cmp.decoded_matches);
    }

    #[test]
    fn charged_histogram_flips_the_winner() {
        // 150 distinct bytes, once each. Static rANS codes them below raw, but its
        // persisted histogram costs more than that gain, so charging the model makes
        // raw (or an adaptive order) the winner. If the implementation forgot to
        // charge the histogram, rANS would win and this test would fail.
        let raw: Vec<u8> = (0..150u16).map(|i| i as u8).collect();
        let choice = choose_stream(&raw);
        let rans_total = choice.size(StreamCodec::Rans);
        assert!(rans_total < usize::MAX, "rANS should be representable");
        assert!(choice.model_bytes > 0, "the rANS histogram must be charged");
        assert!(
            rans_total - choice.model_bytes < raw.len(),
            "rANS symbols alone should beat raw: {} vs {}",
            rans_total - choice.model_bytes,
            raw.len()
        );
        assert!(
            rans_total > raw.len(),
            "the charged model must push rANS past raw: {} vs {}",
            rans_total,
            raw.len()
        );
        assert_ne!(
            choice.codec,
            StreamCodec::Rans,
            "rANS must not win once its model is charged"
        );
    }

    #[test]
    fn best_program_bytes_never_exceeds_raw_serialization() {
        for p in shapes() {
            assert!(best_program_bytes(&p) <= serialize(&p).len());
        }
    }

    #[test]
    fn comparison_is_honest_when_coding_loses() {
        // A one-node program is all container: coding it cannot beat raw varints.
        let p = literal_only();
        let cmp = compare_program(&p);
        if cmp.coded >= cmp.raw {
            assert!(!cmp.wins());
            assert_eq!(cmp.best, cmp.raw);
            assert!(cmp.ratio_per_mille >= 1000);
            assert!(cmp.explain().contains("raw wins"));
        }
        assert_eq!(cmp.best, cmp.raw.min(cmp.coded));
    }

    #[test]
    fn decoding_rejects_truncation() {
        for p in shapes() {
            let bytes = encode_program(&p);
            for cut in 0..bytes.len() {
                assert_eq!(decode_program(&bytes[..cut]), None, "cut at {cut}");
            }
        }
    }

    #[test]
    fn decoding_rejects_a_bad_opcode() {
        // One node claiming operator tag 7, which is outside the seven-operator
        // basis, with no other streams needed to reach the rejection.
        let bytes = container(1, 0, 0, &[(StreamKind::Opcode, vec![7u8])]);
        assert_eq!(decode_program(&bytes), None);
    }

    #[test]
    fn decoding_rejects_a_bad_reference() {
        // Node 0 is a `Concat`; its edge distances decode to target +5, which is
        // outside a one-node program.
        let edge = vec![zigzag(0 - 5) as u8];
        let bytes = container(
            1,
            0,
            0,
            &[
                (StreamKind::Opcode, vec![OP_CONCAT]),
                (StreamKind::Edge, vec![edge[0], edge[0]]),
                (StreamKind::Length, vec![0u8, 0u8]), // the spans
            ],
        );
        assert_eq!(decode_program(&bytes), None);
    }

    #[test]
    fn decoding_rejects_a_cycle() {
        // A self-referencing `Concat` is structurally valid but the VM would reject
        // it as a reference cycle, and so does the decode.
        let p = Program {
            nodes: vec![n(Op::Concat {
                parts: [NodeId(0), NodeId(0)],
            })],
            literals: vec![],
            root: NodeId(0),
        };
        assert_eq!(decode_program(&encode_program(&p)), None);
    }

    #[test]
    fn decoding_rejects_a_depth_violation() {
        // A path one edge longer than the default depth bound.
        let p = deep_concat(Bounds::default().max_depth as u32 + 2);
        let bytes = encode_program(&p);
        assert_eq!(decode_program(&bytes), None);
        // With a raised bound the same bytes decode exactly.
        let mut wide = Bounds::default();
        wide.max_depth = 64;
        assert_eq!(decode_program_with(&bytes, &wide), Some(p));
    }

    #[test]
    fn encoding_is_deterministic() {
        for p in shapes() {
            assert_eq!(encode_program(&p), encode_program(&p));
        }
    }

    #[test]
    fn stream_breakdown_names_the_winner_for_each_present_stream() {
        let p = template_style();
        let (_, cost) = encode_program_with_cost(&p);
        assert!(cost.total > 0);
        assert_eq!(cost.total, cost.header_bytes + cost.streams.iter().map(|s| s.coded_bytes).sum::<usize>());
        // The opcode stream is always present and always nonempty.
        let ops = cost.stream(StreamKind::Opcode).expect("opcode stream");
        assert!(ops.coded_bytes <= ops.raw_bytes);
    }

    #[test]
    fn reported_ratios_are_measured() {
        // Not an assertion about the implementation: it prints the measured ratio
        // so a run with `--nocapture` records it. The claim asserted is only that
        // the realistic skeleton is not made *larger* by coding.
        for p in shapes() {
            let cmp = compare_program(&p);
            eprintln!("[progcodec] {}", cmp.explain());
            for st in &cmp.cost.streams {
                eprintln!(
                    "[progcodec]   {:9} {:7} raw {:6} coded {:6}",
                    st.kind.name(),
                    st.codec.name(),
                    st.raw_bytes,
                    st.coded_bytes
                );
            }
        }
        let realistic = compare_program(&template_style());
        assert!(
            realistic.coded <= realistic.raw,
            "realistic skeleton regressed: {}",
            realistic.explain()
        );
    }
}
