//! The archive container: the scored second half of `S`.
//!
//! Format (little-endian unless stated):
//!
//! ```text
//! offset  size  field
//! 0       4     magic = b"ZNT0"
//! 4       1     method
//! 5       1     tune (optimizer/learning-rate variant; A20)
//! 6       8     coded_len (u64)  -- bytes the context model codes
//! 14      256   symbol permutation (only for methods that permute the alphabet)
//! ...           payload
//! ```
//!
//! The model configuration is a deterministic function of `(method, coded_len)`,
//! so it is not stored; this is correct only while that function is stable,
//! which is why it lives in one place ([`Method::config`]).
//!
//! Optimization Phase A adds mechanisms as *methods* so each is individually
//! ablatable and so the interaction matrix (A28) can be built by adding
//! variants. Every method must reconstruct exactly; the exactness court checks
//! the whole cross-product.

use crate::context::{Cm, InfoMode, ModelConfig};
use crate::entropy::{RangeDecoder, RangeEncoder};

/// Container magic.
pub const MAGIC: &[u8; 4] = b"ZNT0";
/// Fixed header length (excluding any permutation table).
pub const HEADER_LEN: usize = 4 + 1 + 1 + 8;
/// Length of a stored symbol permutation.
pub const PERM_LEN: usize = 256;

/// Upper bound on a declared output length, to bound allocations on corrupt
/// input. The canonical corpus is 10^9 bytes; 2^31 gives headroom for dev
/// slices without permitting an unbounded expansion.
pub const MAX_OUTPUT: u64 = 2_000_000_000;

/// Which refinement a permutation belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermKind {
    None,
    /// A2: frequency-ordered assignment.
    Freq,
    /// A2 negative control: deterministic pseudo-random assignment.
    Random,
}

/// A1.2 case-factorization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(feature = "case-model"), allow(dead_code))]
enum CaseKind {
    None,
    /// Merge lexical identity: lower-case words behind a case marker.
    Merge,
    /// Control: mark case but leave lexical identity untouched.
    MarkOnly,
}

/// A1.1/A26 dynamic word-vocabulary mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(any(feature = "word-token", feature = "word-token2")),
    allow(dead_code)
)]
enum TokenKind {
    None,
    /// Frequency-ranked ids (most frequent word gets id 1).
    Words,
    /// Control: same word set, reversed id assignment.
    Reverse,
    /// v2: escape-extended ids, vocabulary far past 255.
    #[cfg_attr(not(feature = "word-token2"), allow(dead_code))]
    V2,
    /// Phase 4.9: phrase vocabulary with reverse ids.
    #[cfg_attr(not(feature = "phrase"), allow(dead_code))]
    Phrase,
    /// Phase 4.9 control: phrase vocabulary with frequency-ranked ids.
    #[cfg_attr(not(feature = "phrase"), allow(dead_code))]
    PhraseFreq,
    /// Phase 4.10: v1 vocabulary with a front-coded dictionary header.
    #[cfg_attr(not(feature = "dict-front"), allow(dead_code))]
    Front,
    /// Phase 4.8: vocabulary with affix-referenced derived tokens.
    #[cfg_attr(not(feature = "affix-token"), allow(dead_code))]
    Affix,
    /// Phase 10.5: no header — each entry is defined where it is first used.
    #[cfg_attr(not(feature = "first-use"), allow(dead_code))]
    FirstUse,
    /// Phase 10.2: whole-word tokens plus subword units for uncovered words,
    /// with the merge table re-derived from the stored vocabulary.
    #[cfg_attr(not(feature = "subword"), allow(dead_code))]
    Subword,
    /// Phase 10.4: the same vocabulary with **move-to-front** ids, maintained by
    /// both sides from the id sequence alone (no side stream).
    #[cfg_attr(not(feature = "id-order"), allow(dead_code))]
    Mtf,
    /// Phase 10.4 control: move-to-second rather than move-to-front, so a token
    /// used twice in a row keeps a stable id.
    #[cfg_attr(not(feature = "id-order"), allow(dead_code))]
    MoveToSecond,
}

/// Coding method. New mechanisms are added as variants so each is ablatable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Method {
    /// Full floor, no transforms (word + match experts only).
    RawCm = 0,
    /// Floor without the word and word-bigram experts (ablation).
    RawCmNoWord = 1,
    /// Phase 3: structural hoisting + full floor.
    StructHoist = 2,
    /// A2: structural hoisting + frequency-ordered alphabet permutation.
    AlphabetPerm = 3,
    /// A2 negative control: random alphabet permutation.
    AlphabetRandom = 4,
    /// A3: structural hoisting + information inheritance.
    InfoInherit = 5,
    /// A3 negative control: inheritance from an unrelated expert.
    InfoUnrelated = 6,
    /// A2 + A3 combined.
    AlphabetPermInfo = 7,
    /// A17: structural hoisting + previous-line/column expert.
    Column = 8,
    /// A17 negative control: wrong-column expert.
    ColumnShuffled = 9,
    /// A17 null control: no previous-line byte (left + column only).
    ColumnNoLine = 10,
    /// A1.2: case factorization (merge lexical identity behind case markers).
    Case = 11,
    /// A1.2 control: mark case without merging lexical identity.
    CaseMark = 12,
    /// A1.2 on the accepted parent: structural hoisting + column expert + case merge.
    ColumnCase = 13,
    /// A1.2 control on the accepted parent: hoisting + column + case marking only.
    ColumnCaseMark = 14,
    /// A1.1/A26: corpus-derived frequency-ranked word vocabulary.
    WordToken = 15,
    /// A1.1/A26 control: same vocabulary, reversed id assignment.
    WordTokenReverse = 16,
    /// A1.1/A26 on the accepted parent: hoisting + column + frequency-ranked tokens.
    ColumnWordToken = 17,
    /// A1.1/A26 control on the accepted parent: hoisting + column + reversed ids.
    ColumnWordTokenReverse = 18,
    /// A1.1/A26 v2: escape-extended vocabulary, standalone.
    WordToken2 = 19,
    /// A1.1/A26 v2 on the accepted parent: hoisting + column + extended vocabulary.
    ColumnWordToken2 = 20,
    /// Phase 4.1 control: accepted config + a redundant second short (min 6) tier.
    LongMatch6 = 21,
    /// Phase 4.1: accepted config + a long-distance match tier, min 8.
    LongMatch8 = 22,
    /// Phase 4.1: accepted config + a long-distance match tier, min 12.
    LongMatch12 = 23,
    /// Phase 4.1: accepted config + a long-distance match tier, min 16.
    LongMatch16 = 24,
    /// Phase 4.1: accepted config + a long-distance match tier, min 24.
    LongMatch24 = 25,
    /// Phase 4.2: accepted config + a sparse match tier (min 4, gap 1).
    SparseMatch4g1 = 26,
    /// Phase 4.2: accepted config + a sparse match tier (min 6, gap 1).
    SparseMatch6g1 = 27,
    /// Phase 4.2: accepted config + a sparse match tier (min 6, gap 2).
    SparseMatch6g2 = 28,
    /// Phase 4.3: accepted config + repeat-offset predictors.
    RepState1 = 29,
    RepState2 = 30,
    RepState3 = 31,
    RepState4 = 32,
    /// Phase 4.4: accepted config + a matched-literal expert.
    MatchByte = 33,
    /// Phase 4.4 control: the same expert with the predicted byte removed.
    MatchByteConst = 34,
    /// Phase 4 composite: long-match8 + sparse4g1 + matched-literal on the accepted parent.
    Phase4 = 35,
    /// Phase 4.5: accepted config + distance-conditioned match confidence.
    DistMatch = 36,
    /// Phase 4.6: standalone stem / root+affix transform (screened vs the floor).
    Stem = 37,
    /// Phase 4.7: accepted config + word-class context expert.
    WordClass = 38,
    /// Phase 4.7 control: the same expert keyed only on "inside a word".
    WordClassConst = 39,
    /// Phase 4.9: accepted config + phrase vocabulary (reverse ids).
    ColumnWordTokenPhrase = 40,
    /// Phase 4.9 control: phrase vocabulary with frequency-ranked ids.
    ColumnWordTokenPhraseFreq = 41,
    /// Phase 4.10: accepted config + front-coded dictionary header.
    ColumnWordTokenFront = 42,
    /// Phase 4.8: accepted config + affix-referenced derived tokens.
    ColumnWordTokenAffix = 43,
    /// Phase 5.1: standalone RePair grammar transform.
    Grammar = 44,
    /// Phase 5.3: grammar with a move-to-front rank-coded symbol stream.
    GrammarMtf = 45,
    /// Phase 5.4: grammar induced by maximal repeats (MR-RePair).
    GrammarMr = 46,
    /// Phase 5.6: grammar with the rule table reordered by first use (rank state).
    GrammarRrank = 47,
    /// Phase 5.5: grammar with first-use inline productions.
    GrammarFirstUse = 48,
    /// Phase 5.9: scalable one-shot grammar induction (large-corpus feasibility).
    GrammarOneShot = 49,
    /// Phase 5.8: LZ-Begin-End factorization (A15).
    Lzbe = 50,
    /// Phase 6.1: accepted config + indirect (bit-history state) experts.
    StateMap = 51,
    /// Phase 6.1 control: the same mixer width with direct order experts.
    StateMapCtl = 52,
    /// Phase 6.1: replace direct orders 2/4/6 with state-map experts (same width).
    StateMapRep = 53,
    /// Phase 6.4: accepted config + an extra SSE/APM stage keyed on order-2.
    Sse3 = 54,
    /// Phase 6.4 negative control: the same stage keyed on an uncorrelated byte.
    Sse3Ctl = 55,
    /// Phase 6.5: accepted config + a sparse (gapped) context expert, order 4 gap 1.
    Sparse4g1 = 56,
    /// Phase 6.5 width control: accepted config + a direct order-4 expert.
    Sparse4g1Ctl = 57,
    /// Phase 6.5 representation control: replace the direct order-4 expert with
    /// the sparse order-4 gap-1 expert (same mixer width).
    Sparse4g1Rep = 58,
    /// Phase 6.2: accepted config + an indirect context model (order 2, history 2).
    Icm = 59,
    /// Phase 6.2 negative control: indirect machine with a permuted slot key.
    IcmCtl = 60,
    /// Phase 6.6: the second-order (chained) indirect context model.
    IndirectChain = 61,
    /// Phase 6.6: indirect over a deeper source context (order 5, history 2).
    IndirectDeep = 62,
    /// Phase 6.7: checksum-verified context slots (collision control).
    Collision = 63,
    /// Phase 6.7 negative control: reset slots on a deliberately wrong checksum.
    CollisionCtl = 64,
    /// Phase 6.8: a bounded PPM-style byte model as one expert.
    Ppm = 65,
    /// Phase 6.8 control: the same expert with escape/backoff disabled.
    PpmCtl = 66,
    /// Phase 6.10: a stem-folded word context model (not a transform).
    StemModel = 67,
    /// Phase 6.10 control: the same expert without stem folding.
    StemModelCtl = 68,
    /// Phase 6.3: an indirect (state-keyed) SSE stage.
    Isse = 69,
    /// Phase 6.3 control: a state-invariant SSE stage of the same width.
    IsseCtl = 70,
    /// Phase 6 parent: accepted config + the two adopted spine mechanisms
    /// (indirect bit-history state experts and the extra SSE stage).
    Phase6 = 71,
    /// Phase 6 final spine: Phase6 + the PPM-C expert, with the four direct
    /// order experts that PPM subsumes (4/8/12/16) pruned by measurement.
    Spine = 72,
    /// Phase 7 control: the accepted config with the reorder machinery at the
    /// identity order (isolates machinery cost from the ordering effect).
    ReorderId = 73,
    /// Phase 7.2: title order.
    ReorderTitle = 74,
    /// Phase 7.3: page-size order.
    ReorderSize = 75,
    /// Phase 7.4: namespace/structural order.
    ReorderStruct = 76,
    /// Phase 7.5: MinHash/LSH content-similarity order.
    ReorderMinHash = 77,
    /// Phase 7.6: windowed greedy nearest-neighbour order.
    ReorderGreedy = 78,
    /// Phase 7.7: boilerplate-signature (template/category) order.
    ReorderTemplate = 79,
    /// Phase 7 negative control: deterministic shuffle (locality destroyed).
    ReorderShuffle = 80,
    /// Phase 7.8: first-category structural order.
    ReorderCategory = 81,
    /// Phase 7.8: first-template structural order.
    ReorderTemplateKey = 82,
    /// Phase 7.8: all-categories (category-set) structural order.
    ReorderCategorySet = 83,
    /// Phase 7.8: combined category-set + template-set order.
    ReorderFull = 84,
    /// Phase 7.8: combined order with a residual (local-model novelty) tiebreak.
    ReorderFullResidual = 85,
    /// Phase 8: accepted config + the learned residual corrector (frozen weights).
    Residual = 86,
    /// Phase 8.7 control: the same corrector with permuted weights.
    ResidualCtl = 87,
    /// Phase 10.4: the accepted config with move-to-front token ids.
    #[cfg_attr(not(feature = "id-order"), allow(dead_code))]
    ResidualMtf = 88,
    /// Phase 10.4 control: the accepted config with move-to-second token ids.
    #[cfg_attr(not(feature = "id-order"), allow(dead_code))]
    ResidualMoveToSecond = 89,
    /// Phase 10: the accepted config with a vocabulary selected by the model's
    /// **measured** cost for each candidate word, instead of the shipped raw-byte
    /// count heuristic. Encoder-side only; the decoder reads the stored
    /// dictionary and is unchanged.
    #[cfg_attr(not(feature = "vocab-price"), allow(dead_code))]
    ResidualPriced = 90,
    /// Phase 10: the accepted config with the vocabulary words the model prices
    /// as unprofitable **not substituted** (ids unchanged).
    #[cfg_attr(not(feature = "vocab-price"), allow(dead_code))]
    ResidualPriceFilter = 91,
    /// Phase 10.3: the accepted config with the **affix** representation family —
    /// words composed as root+affix tokens — measured on the mature composite.
    /// Phase 4.8 rejected the same family on a much weaker parent, so this is a
    /// re-measurement of a *family*, not a re-litigation of that rejection.
    #[cfg_attr(not(feature = "affix-token"), allow(dead_code))]
    ResidualAffix = 92,
    /// Phase 10.5: the accepted composite with **first-use inline definitions**
    /// instead of a dictionary header. Ids are preserved, so the identity axis is
    /// untouched; the change is where a definition lives, not what it is.
    #[cfg_attr(not(feature = "first-use"), allow(dead_code))]
    ResidualFirstUse = 93,
    /// Phase 10.2: the accepted composite with subword composition for the words
    /// the whole-word vocabulary does not cover.
    #[cfg_attr(not(feature = "subword"), allow(dead_code))]
    ResidualSubword = 94,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::RawCm => "rawcm",
            Method::RawCmNoWord => "rawcm-noword",
            Method::StructHoist => "struct-hoist",
            Method::AlphabetPerm => "alphabet-perm",
            Method::AlphabetRandom => "alphabet-random",
            Method::InfoInherit => "info-inherit",
            Method::InfoUnrelated => "info-unrelated",
            Method::AlphabetPermInfo => "alphabet-perm+info-inherit",
            Method::Column => "column",
            Method::ColumnShuffled => "column-shuffled",
            Method::ColumnNoLine => "column-noline",
            Method::Case => "case",
            Method::CaseMark => "case-mark",
            Method::ColumnCase => "column-case",
            Method::ColumnCaseMark => "column-case-mark",
            Method::WordToken => "word-token",
            Method::WordTokenReverse => "word-token-reverse",
            Method::ColumnWordToken => "column-word-token",
            Method::ColumnWordTokenReverse => "column-word-token-reverse",
            Method::WordToken2 => "word-token2",
            Method::ColumnWordToken2 => "column-word-token2",
            Method::LongMatch6 => "long-match6",
            Method::LongMatch8 => "long-match8",
            Method::LongMatch12 => "long-match12",
            Method::LongMatch16 => "long-match16",
            Method::LongMatch24 => "long-match24",
            Method::SparseMatch4g1 => "sparse-match4g1",
            Method::SparseMatch6g1 => "sparse-match6g1",
            Method::SparseMatch6g2 => "sparse-match6g2",
            Method::RepState1 => "rep-state1",
            Method::RepState2 => "rep-state2",
            Method::RepState3 => "rep-state3",
            Method::RepState4 => "rep-state4",
            Method::MatchByte => "match-byte",
            Method::MatchByteConst => "match-byte-const",
            Method::Phase4 => "phase4",
            Method::DistMatch => "dist-match",
            Method::Stem => "stem",
            Method::WordClass => "word-class",
            Method::WordClassConst => "word-class-const",
            Method::ColumnWordTokenPhrase => "column-word-token-phrase",
            Method::ColumnWordTokenPhraseFreq => "column-word-token-phrase-freq",
            Method::ColumnWordTokenFront => "column-word-token-front",
            Method::ColumnWordTokenAffix => "column-word-token-affix",
            Method::Grammar => "grammar",
            Method::GrammarMtf => "grammar-mtf",
            Method::GrammarMr => "grammar-mr",
            Method::GrammarRrank => "grammar-rrank",
            Method::GrammarFirstUse => "grammar-first-use",
            Method::GrammarOneShot => "grammar-oneshot",
            Method::Lzbe => "lzbe",
            Method::StateMap => "state-map",
            Method::StateMapCtl => "state-map-ctl",
            Method::StateMapRep => "state-map-rep",
            Method::Sse3 => "sse-3",
            Method::Sse3Ctl => "sse-3-ctl",
            Method::Sparse4g1 => "sparse4g1",
            Method::Sparse4g1Ctl => "sparse4g1-ctl",
            Method::Sparse4g1Rep => "sparse4g1-rep",
            Method::Icm => "icm",
            Method::IcmCtl => "icm-ctl",
            Method::IndirectChain => "indirect-chain",
            Method::IndirectDeep => "indirect-deep",
            Method::Collision => "collision",
            Method::CollisionCtl => "collision-ctl",
            Method::Ppm => "ppm",
            Method::PpmCtl => "ppm-ctl",
            Method::StemModel => "stem-model",
            Method::StemModelCtl => "stem-model-ctl",
            Method::Isse => "isse",
            Method::IsseCtl => "isse-ctl",
            Method::Phase6 => "phase6",
            Method::Spine => "spine",
            Method::ReorderId => "reorder-id",
            Method::ReorderTitle => "reorder-title",
            Method::ReorderSize => "reorder-size",
            Method::ReorderStruct => "reorder-struct",
            Method::ReorderMinHash => "reorder-minhash",
            Method::ReorderGreedy => "reorder-greedy",
            Method::ReorderTemplate => "reorder-template",
            Method::ReorderShuffle => "reorder-shuffle",
            Method::ReorderCategory => "reorder-category",
            Method::ReorderTemplateKey => "reorder-template-key",
            Method::ReorderCategorySet => "reorder-category-set",
            Method::ReorderFull => "reorder-full",
            Method::ReorderFullResidual => "reorder-full-residual",
            Method::Residual => "residual",
            Method::ResidualCtl => "residual-ctl",
            Method::ResidualMtf => "residual-mtf",
            Method::ResidualMoveToSecond => "residual-move-to-second",
            Method::ResidualPriced => "residual-priced",
            Method::ResidualPriceFilter => "residual-price-filter",
            Method::ResidualAffix => "residual-affix",
            Method::ResidualFirstUse => "residual-first-use",
            Method::ResidualSubword => "residual-subword",
        }
    }

    pub fn from_name(s: &str) -> Option<Method> {
        Some(match s {
            "rawcm" => Method::RawCm,
            "rawcm-noword" => Method::RawCmNoWord,
            "struct-hoist" => Method::StructHoist,
            "alphabet-perm" => Method::AlphabetPerm,
            "alphabet-random" => Method::AlphabetRandom,
            "info-inherit" => Method::InfoInherit,
            "info-unrelated" => Method::InfoUnrelated,
            "alphabet-perm+info-inherit" => Method::AlphabetPermInfo,
            "column" => Method::Column,
            "column-shuffled" => Method::ColumnShuffled,
            "column-noline" => Method::ColumnNoLine,
            "case" => Method::Case,
            "case-mark" => Method::CaseMark,
            "column-case" => Method::ColumnCase,
            "column-case-mark" => Method::ColumnCaseMark,
            "word-token" => Method::WordToken,
            "word-token-reverse" => Method::WordTokenReverse,
            "column-word-token" => Method::ColumnWordToken,
            "column-word-token-reverse" => Method::ColumnWordTokenReverse,
            "word-token2" => Method::WordToken2,
            "column-word-token2" => Method::ColumnWordToken2,
            "long-match6" => Method::LongMatch6,
            "long-match8" => Method::LongMatch8,
            "long-match12" => Method::LongMatch12,
            "long-match16" => Method::LongMatch16,
            "long-match24" => Method::LongMatch24,
            "sparse-match4g1" => Method::SparseMatch4g1,
            "sparse-match6g1" => Method::SparseMatch6g1,
            "sparse-match6g2" => Method::SparseMatch6g2,
            "rep-state1" => Method::RepState1,
            "rep-state2" => Method::RepState2,
            "rep-state3" => Method::RepState3,
            "rep-state4" => Method::RepState4,
            "match-byte" => Method::MatchByte,
            "match-byte-const" => Method::MatchByteConst,
            "phase4" => Method::Phase4,
            "dist-match" => Method::DistMatch,
            "stem" => Method::Stem,
            "word-class" => Method::WordClass,
            "word-class-const" => Method::WordClassConst,
            "column-word-token-phrase" => Method::ColumnWordTokenPhrase,
            "column-word-token-phrase-freq" => Method::ColumnWordTokenPhraseFreq,
            "column-word-token-front" => Method::ColumnWordTokenFront,
            "column-word-token-affix" => Method::ColumnWordTokenAffix,
            "grammar" => Method::Grammar,
            "grammar-mtf" => Method::GrammarMtf,
            "grammar-mr" => Method::GrammarMr,
            "grammar-rrank" => Method::GrammarRrank,
            "grammar-first-use" => Method::GrammarFirstUse,
            "grammar-oneshot" => Method::GrammarOneShot,
            "lzbe" => Method::Lzbe,
            "state-map" => Method::StateMap,
            "state-map-ctl" => Method::StateMapCtl,
            "state-map-rep" => Method::StateMapRep,
            "sse-3" => Method::Sse3,
            "sse-3-ctl" => Method::Sse3Ctl,
            "sparse4g1" => Method::Sparse4g1,
            "sparse4g1-ctl" => Method::Sparse4g1Ctl,
            "sparse4g1-rep" => Method::Sparse4g1Rep,
            "icm" => Method::Icm,
            "icm-ctl" => Method::IcmCtl,
            "indirect-chain" => Method::IndirectChain,
            "indirect-deep" => Method::IndirectDeep,
            "collision" => Method::Collision,
            "collision-ctl" => Method::CollisionCtl,
            "ppm" => Method::Ppm,
            "ppm-ctl" => Method::PpmCtl,
            "stem-model" => Method::StemModel,
            "stem-model-ctl" => Method::StemModelCtl,
            "isse" => Method::Isse,
            "isse-ctl" => Method::IsseCtl,
            "phase6" => Method::Phase6,
            "spine" => Method::Spine,
            "reorder-id" => Method::ReorderId,
            "reorder-title" => Method::ReorderTitle,
            "reorder-size" => Method::ReorderSize,
            "reorder-struct" => Method::ReorderStruct,
            "reorder-minhash" => Method::ReorderMinHash,
            "reorder-greedy" => Method::ReorderGreedy,
            "reorder-template" => Method::ReorderTemplate,
            "reorder-shuffle" => Method::ReorderShuffle,
            "reorder-category" => Method::ReorderCategory,
            "reorder-template-key" => Method::ReorderTemplateKey,
            "reorder-category-set" => Method::ReorderCategorySet,
            "reorder-full" => Method::ReorderFull,
            "reorder-full-residual" => Method::ReorderFullResidual,
            "residual" => Method::Residual,
            "residual-ctl" => Method::ResidualCtl,
            "residual-mtf" => Method::ResidualMtf,
            "residual-move-to-second" => Method::ResidualMoveToSecond,
            "residual-priced" => Method::ResidualPriced,
            "residual-price-filter" => Method::ResidualPriceFilter,
            "residual-affix" => Method::ResidualAffix,
            "residual-first-use" => Method::ResidualFirstUse,
            "residual-subword" => Method::ResidualSubword,
            _ => return None,
        })
    }

    /// All methods, for exhaustive exactness testing.
    pub const ALL: [Method; 95] = [
        Method::RawCm,
        Method::RawCmNoWord,
        Method::StructHoist,
        Method::AlphabetPerm,
        Method::AlphabetRandom,
        Method::InfoInherit,
        Method::InfoUnrelated,
        Method::AlphabetPermInfo,
        Method::Column,
        Method::ColumnShuffled,
        Method::ColumnNoLine,
        Method::Case,
        Method::CaseMark,
        Method::ColumnCase,
        Method::ColumnCaseMark,
        Method::WordToken,
        Method::WordTokenReverse,
        Method::ColumnWordToken,
        Method::ColumnWordTokenReverse,
        Method::WordToken2,
        Method::ColumnWordToken2,
        Method::LongMatch6,
        Method::LongMatch8,
        Method::LongMatch12,
        Method::LongMatch16,
        Method::LongMatch24,
        Method::SparseMatch4g1,
        Method::SparseMatch6g1,
        Method::SparseMatch6g2,
        Method::RepState1,
        Method::RepState2,
        Method::RepState3,
        Method::RepState4,
        Method::MatchByte,
        Method::MatchByteConst,
        Method::Phase4,
        Method::DistMatch,
        Method::Stem,
        Method::WordClass,
        Method::WordClassConst,
        Method::ColumnWordTokenPhrase,
        Method::ColumnWordTokenPhraseFreq,
        Method::ColumnWordTokenFront,
        Method::ColumnWordTokenAffix,
        Method::Grammar,
        Method::GrammarMtf,
        Method::GrammarMr,
        Method::GrammarRrank,
        Method::GrammarFirstUse,
        Method::GrammarOneShot,
        Method::Lzbe,
        Method::StateMap,
        Method::StateMapCtl,
        Method::StateMapRep,
        Method::Sse3,
        Method::Sse3Ctl,
        Method::Sparse4g1,
        Method::Sparse4g1Ctl,
        Method::Sparse4g1Rep,
        Method::Icm,
        Method::IcmCtl,
        Method::IndirectChain,
        Method::IndirectDeep,
        Method::Collision,
        Method::CollisionCtl,
        Method::Ppm,
        Method::PpmCtl,
        Method::StemModel,
        Method::StemModelCtl,
        Method::Isse,
        Method::IsseCtl,
        Method::Phase6,
        Method::Spine,
        Method::ReorderId,
        Method::ReorderTitle,
        Method::ReorderSize,
        Method::ReorderStruct,
        Method::ReorderMinHash,
        Method::ReorderGreedy,
        Method::ReorderTemplate,
        Method::ReorderShuffle,
        Method::ReorderCategory,
        Method::ReorderTemplateKey,
        Method::ReorderCategorySet,
        Method::ReorderFull,
        Method::ReorderFullResidual,
        Method::Residual,
        Method::ResidualCtl,
        Method::ResidualMtf,
        Method::ResidualMoveToSecond,
        Method::ResidualPriced,
        Method::ResidualPriceFilter,
        Method::ResidualAffix,
        Method::ResidualFirstUse,
        Method::ResidualSubword,
    ];

    /// Methods that extend the **accepted Phase-4 composite parent** unchanged:
    /// structural hoist, reversed word vocabulary, the previous-line/column
    /// expert, the long-match8 and sparse-match4g1 tiers, and the matched-literal
    /// expert. Every Phase 6 mechanism is measured on exactly this base so that
    /// `ΔS` attributes the mechanism and not a missing component of the parent.
    fn on_phase4_parent(self) -> bool {
        matches!(
            self,
            Method::Phase4
                | Method::StateMap
                | Method::StateMapCtl
                | Method::StateMapRep
                | Method::Sse3
                | Method::Sse3Ctl
                | Method::Sparse4g1
                | Method::Sparse4g1Ctl
                | Method::Sparse4g1Rep
                | Method::Icm
                | Method::IcmCtl
                | Method::IndirectChain
                | Method::IndirectDeep
                | Method::Collision
                | Method::CollisionCtl
                | Method::Ppm
                | Method::PpmCtl
                | Method::StemModel
                | Method::StemModelCtl
                | Method::Isse
                | Method::IsseCtl
                | Method::Phase6
                | Method::Spine
                | Method::ReorderId
                | Method::ReorderTitle
                | Method::ReorderSize
                | Method::ReorderStruct
                | Method::ReorderMinHash
                | Method::ReorderGreedy
                | Method::ReorderTemplate
                | Method::ReorderShuffle
                | Method::ReorderCategory
                | Method::ReorderTemplateKey
                | Method::ReorderCategorySet
                | Method::ReorderFull
                | Method::ReorderFullResidual
                | Method::Residual
                | Method::ResidualCtl
                | Method::ResidualMtf
                | Method::ResidualMoveToSecond
                | Method::ResidualPriced
                | Method::ResidualPriceFilter
                | Method::ResidualAffix
                | Method::ResidualFirstUse
                | Method::ResidualSubword
        )
    }

    /// Methods measured on the Phase 6 parent: the accepted config plus the two
    /// adopted spine mechanisms (state-map experts and the extra SSE stage).
    fn base6(self) -> bool {
        matches!(
            self,
            Method::Phase6
                | Method::Spine
                | Method::ReorderId
                | Method::ReorderTitle
                | Method::ReorderSize
                | Method::ReorderStruct
                | Method::ReorderMinHash
                | Method::ReorderGreedy
                | Method::ReorderTemplate
                | Method::ReorderShuffle
                | Method::ReorderCategory
                | Method::ReorderTemplateKey
                | Method::ReorderCategorySet
                | Method::ReorderFull
                | Method::ReorderFullResidual
                | Method::Residual
                | Method::ResidualCtl
                | Method::Collision
                | Method::CollisionCtl
                | Method::Ppm
                | Method::PpmCtl
                | Method::StemModel
                | Method::StemModelCtl
                | Method::Isse
                | Method::IsseCtl
        )
    }

    /// Whether this method runs the structural-hoisting transform.
    fn hoists(self) -> bool {
        let wants = self.on_phase4_parent()
            || matches!(
                self,
                Method::StructHoist
                    | Method::AlphabetPerm
                    | Method::AlphabetRandom
                    | Method::InfoInherit
                    | Method::InfoUnrelated
                    | Method::AlphabetPermInfo
                    | Method::Column
                    | Method::ColumnShuffled
                    | Method::ColumnNoLine
                    | Method::ColumnCase
                    | Method::ColumnCaseMark
                    | Method::ColumnWordToken
                    | Method::ColumnWordTokenReverse
                    | Method::ColumnWordToken2
                    | Method::LongMatch6
                    | Method::LongMatch8
                    | Method::LongMatch12
                    | Method::LongMatch16
                    | Method::LongMatch24
                    | Method::SparseMatch4g1
                    | Method::SparseMatch6g1
                    | Method::SparseMatch6g2
                    | Method::RepState1
                    | Method::RepState2
                    | Method::RepState3
                    | Method::RepState4
                    | Method::MatchByte
                    | Method::MatchByteConst
                    | Method::Phase4
                    | Method::DistMatch
                    | Method::WordClass
                    | Method::WordClassConst
                    | Method::ColumnWordTokenPhrase
                    | Method::ColumnWordTokenPhraseFreq
                    | Method::ColumnWordTokenFront
                    | Method::ColumnWordTokenAffix
            );
        cfg!(feature = "struct-hoist") && wants
    }

    /// Which alphabet permutation this method applies, if any.
    fn perm(self) -> PermKind {
        if !cfg!(feature = "alphabet-perm") {
            return PermKind::None;
        }
        match self {
            Method::AlphabetPerm | Method::AlphabetPermInfo => PermKind::Freq,
            Method::AlphabetRandom => PermKind::Random,
            _ => PermKind::None,
        }
    }

    /// A3 information-inheritance mode.
    fn info(self) -> InfoMode {
        if !cfg!(feature = "info-inherit") {
            return InfoMode::None;
        }
        match self {
            Method::InfoInherit | Method::AlphabetPermInfo => InfoMode::Inherit,
            Method::InfoUnrelated => InfoMode::Unrelated,
            _ => InfoMode::None,
        }
    }

    /// A1.2 case-factorization mode.
    #[cfg_attr(not(feature = "case-model"), allow(dead_code))]
    fn case_kind(self) -> CaseKind {
        if !cfg!(feature = "case-model") {
            return CaseKind::None;
        }
        match self {
            Method::Case => CaseKind::Merge,
            Method::CaseMark => CaseKind::MarkOnly,
            Method::ColumnCase => CaseKind::Merge,
            Method::ColumnCaseMark => CaseKind::MarkOnly,
            _ => CaseKind::None,
        }
    }

    /// A1.1/A26 dynamic word-vocabulary mode.
    #[cfg_attr(
        not(any(feature = "word-token", feature = "word-token2")),
        allow(dead_code)
    )]
    /// Phase 10: whether this method chooses its tokenizer vocabulary from the
    /// model's **measured** per-word costs rather than the shipped count
    /// heuristic. The choice is encoder-side and travels inside the stream's own
    /// dictionary, so it can never affect `decode`.
    ///
    /// Without the feature the predicate is false and `ResidualPriced` collapses
    /// onto the accepted configuration, which is exactly what an ablation needs.
    #[cfg(feature = "vocab-price")]
    fn priced_vocab(self) -> bool {
        matches!(self, Method::ResidualPriced | Method::ResidualPriceFilter)
    }

    /// Phase 10: which vocabulary policy this method applies. Only meaningful
    /// when [`Method::priced_vocab`] is true.
    #[cfg(feature = "vocab-price")]
    fn vocab_policy(self) -> VocabPolicy {
        match self {
            Method::ResidualPriceFilter => VocabPolicy::Filter,
            Method::ResidualPriced => VocabPolicy::Rerank,
            _ => VocabPolicy::Shipped,
        }
    }

    #[cfg(not(feature = "vocab-price"))]
    #[cfg_attr(not(feature = "vocab-price"), allow(dead_code))]
    fn priced_vocab(self) -> bool {
        let _ = self;
        false
    }

    fn token_kind(self) -> TokenKind {
        #[cfg(feature = "word-token")]
        {
            // Phase 10.4: check the representation variants *before* the
            // composite shortcut below, which would otherwise pin every
            // Phase-4-parent method to `Reverse`.
            #[cfg(feature = "id-order")]
            match self {
                Method::ResidualMtf => return TokenKind::Mtf,
                Method::ResidualMoveToSecond => return TokenKind::MoveToSecond,
                _ => {}
            }
            // Phase 10: the priced vocabulary changes *which* words are tokens,
            // not how ids are assigned, so it inherits the accepted `Reverse` id
            // mode and keeps the identity axis fixed while membership moves.
            if self.priced_vocab() {
                return TokenKind::Reverse;
            }
            // Phase 10.3: the affix family on the composite, checked before the
            // `on_phase4_parent` shortcut for the same reason as the others.
            #[cfg(feature = "affix-token")]
            if matches!(self, Method::ResidualAffix) {
                return TokenKind::Affix;
            }
            // Phase 10.5: first-use definitions.
            #[cfg(feature = "first-use")]
            if matches!(self, Method::ResidualFirstUse) {
                return TokenKind::FirstUse;
            }
            // Phase 10.2: subword composition.
            #[cfg(feature = "subword")]
            if matches!(self, Method::ResidualSubword) {
                return TokenKind::Subword;
            }
            if self.on_phase4_parent() {
                return TokenKind::Reverse;
            }
            match self {
                Method::WordToken | Method::ColumnWordToken => return TokenKind::Words,
                Method::WordTokenReverse
                | Method::ColumnWordTokenReverse
                | Method::LongMatch6
                | Method::LongMatch8
                | Method::LongMatch12
                | Method::LongMatch16
                | Method::LongMatch24
                | Method::SparseMatch4g1
                | Method::SparseMatch6g1
                | Method::SparseMatch6g2
                | Method::RepState1
                | Method::RepState2
                | Method::RepState3
                | Method::RepState4
                | Method::MatchByte
                | Method::MatchByteConst
                | Method::Phase4
                | Method::DistMatch
                | Method::WordClass
                | Method::WordClassConst => return TokenKind::Reverse,
                _ => {}
            }
        }
        #[cfg(feature = "word-token2")]
        match self {
            Method::WordToken2 | Method::ColumnWordToken2 => return TokenKind::V2,
            _ => {}
        }
        #[cfg(feature = "phrase")]
        match self {
            Method::ColumnWordTokenPhrase => return TokenKind::Phrase,
            Method::ColumnWordTokenPhraseFreq => return TokenKind::PhraseFreq,
            _ => {}
        }
        #[cfg(feature = "dict-front")]
        match self {
            Method::ColumnWordTokenFront => return TokenKind::Front,
            _ => {}
        }
        #[cfg(feature = "affix-token")]
        match self {
            Method::ColumnWordTokenAffix => return TokenKind::Affix,
            _ => {}
        }
        let _ = self;
        TokenKind::None
    }

    /// Phase 4.1: the long-distance match tier's minimum length, if any.
    #[cfg_attr(not(feature = "long-match"), allow(dead_code))]
    fn match2_min(self) -> Option<usize> {
        if !cfg!(feature = "long-match") {
            return None;
        }
        if self.on_phase4_parent() {
            return Some(8);
        }
        match self {
            Method::LongMatch6 => Some(6),
            Method::LongMatch8 => Some(8),
            Method::LongMatch12 => Some(12),
            Method::LongMatch16 => Some(16),
            Method::LongMatch24 => Some(24),
            Method::Phase4 => Some(8),
            _ => None,
        }
    }

    /// Phase 4.2: the sparse match tier's (min_len, gap), if any.
    #[cfg_attr(not(feature = "sparse-match"), allow(dead_code))]
    fn sparse_tier(self) -> Option<(usize, usize)> {
        if !cfg!(feature = "sparse-match") {
            return None;
        }
        if self.on_phase4_parent() {
            return Some((4, 1));
        }
        match self {
            Method::SparseMatch4g1 => Some((4, 1)),
            Method::SparseMatch6g1 => Some((6, 1)),
            Method::SparseMatch6g2 => Some((6, 2)),
            Method::Phase4 => Some((4, 1)),
            _ => None,
        }
    }

    /// Phase 4.3: number of repeat-offset predictors.
    #[cfg_attr(not(feature = "rep-state"), allow(dead_code))]
    fn rep_offsets(self) -> usize {
        if !cfg!(feature = "rep-state") {
            return 0;
        }
        match self {
            Method::RepState1 => 1,
            Method::RepState2 => 2,
            Method::RepState3 => 3,
            Method::RepState4 => 4,
            _ => 0,
        }
    }

    /// Phase 4.4: matched-literal expert; `Some(true)` is the constant control.
    #[cfg_attr(not(feature = "match-byte"), allow(dead_code))]
    fn match_byte_kind(self) -> Option<bool> {
        if !cfg!(feature = "match-byte") {
            return None;
        }
        if self.on_phase4_parent() {
            return Some(false);
        }
        match self {
            Method::MatchByte => Some(false),
            Method::MatchByteConst => Some(true),
            Method::Phase4 => Some(false),
            _ => None,
        }
    }

    /// Phase 4.5: distance-conditioned match confidence.
    #[cfg_attr(not(feature = "dist-match"), allow(dead_code))]
    fn dist_match(self) -> bool {
        cfg!(feature = "dist-match") && matches!(self, Method::DistMatch)
    }

    /// Phase 4.6: whether the stem transform runs.
    #[cfg_attr(not(feature = "stem"), allow(dead_code))]
    fn stems(self) -> bool {
        cfg!(feature = "stem") && matches!(self, Method::Stem)
    }

    /// Phase 4.7: word-class expert; `Some(true)` is the constant control.
    #[cfg_attr(not(feature = "word-class"), allow(dead_code))]
    fn word_class_kind(self) -> Option<bool> {
        if !cfg!(feature = "word-class") {
            return None;
        }
        match self {
            Method::WordClass => Some(false),
            Method::WordClassConst => Some(true),
            _ => None,
        }
    }

    /// Phase 5: the grammar mode, if any.
    #[cfg(feature = "grammar")]
    fn grammar_mode(self) -> Option<GrammarMode> {
        match self {
            Method::Grammar => Some(GrammarMode::Verbatim),
            Method::GrammarMtf => Some(GrammarMode::Mtf),
            Method::GrammarMr => Some(GrammarMode::Mr),
            Method::GrammarRrank => Some(GrammarMode::Rrank),
            Method::GrammarFirstUse => Some(GrammarMode::FirstUse),
            Method::GrammarOneShot => Some(GrammarMode::OneShot),
            _ => None,
        }
    }

    /// Phase 5.8: whether the LZBE transform runs.
    #[cfg_attr(not(feature = "grammar"), allow(dead_code))]
    fn lzbe(self) -> bool {
        cfg!(feature = "grammar") && matches!(self, Method::Lzbe)
    }

    /// Phase 6.1: `Some(false)` = state experts, `Some(true)` = direct control.
    #[cfg_attr(not(feature = "state-map"), allow(dead_code))]
    fn state_map_kind(self) -> Option<bool> {
        if !cfg!(feature = "state-map") {
            return None;
        }
        match self {
            Method::StateMap => Some(false),
            Method::StateMapCtl => Some(true),
            _ => None,
        }
    }

    /// Phase 6.1: replace direct orders with state-map experts.
    #[cfg_attr(not(feature = "state-map"), allow(dead_code))]
    fn state_map_rep(self) -> bool {
        cfg!(feature = "state-map") && matches!(self, Method::StateMapRep)
    }

    /// Phase 7: the encoder-side order (None = no reorder). The decoder never
    /// needs the order — it always restores by sorting on the page id.
    #[cfg(feature = "reorder")]
    fn reorder_kind(self) -> Option<crate::reorder::Order> {
        use crate::reorder::Order;
        match self {
            Method::ReorderId => Some(Order::Identity),
            Method::ReorderTitle => Some(Order::Title),
            Method::ReorderSize => Some(Order::Size),
            Method::ReorderStruct => Some(Order::Struct),
            Method::ReorderMinHash => Some(Order::MinHash),
            Method::ReorderGreedy => Some(Order::Greedy),
            Method::ReorderTemplate => Some(Order::Template),
            Method::ReorderShuffle => Some(Order::Shuffle),
            Method::ReorderCategory => Some(Order::Category),
            Method::ReorderTemplateKey => Some(Order::TemplateKey),
            Method::ReorderCategorySet => Some(Order::CategorySet),
            Method::ReorderFull => Some(Order::Full),
            Method::ReorderFullResidual => Some(Order::FullResidual),
            Method::Residual | Method::ResidualCtl => Some(Order::Full),
            // Phase 10.4: the same composite and the same article layout; only
            // the token id assignment differs, so the comparison is clean.
            Method::ResidualMtf | Method::ResidualMoveToSecond => Some(Order::Full),
            // Phase 10: same composite, same article layout, same id mode; only the
            // vocabulary *membership* changes.
            Method::ResidualPriced => Some(Order::Full),
            Method::ResidualPriceFilter => Some(Order::Full),
            Method::ResidualAffix => Some(Order::Full),
            Method::ResidualFirstUse => Some(Order::Full),
            Method::ResidualSubword => Some(Order::Full),
            _ => None,
        }
    }

    /// Phase 7: whether a decoded stream must be restored by a page-id sort.
    /// Equal to `reorder_kind().is_some()`; kept separate so the decoder's path
    /// is a single cheap predicate.
    #[cfg(feature = "reorder")]
    fn reorders(self) -> bool {
        self.reorder_kind().is_some()
    }

    #[cfg(not(feature = "reorder"))]
    #[cfg_attr(not(feature = "reorder"), allow(dead_code))]
    fn reorders(self) -> bool {
        let _ = self;
        false
    }

    /// Phase 7: the method to fall back to when the free-restoration
    /// precondition fails (the corpus is not page-id ascending). The accepted
    /// configuration without the reorder.
    #[cfg(feature = "reorder")]
    fn reorder_parent(self) -> Method {
        let _ = self;
        Method::Sse3
    }

    /// Phase 6.4: `Some(false)` = order-2 key, `Some(true)` = distant control.
    #[cfg_attr(not(feature = "sse-3"), allow(dead_code))]
    fn sse3_kind(self) -> Option<bool> {
        if !cfg!(feature = "sse-3") {
            return None;
        }
        match self {
            Method::Sse3 => Some(false),
            Method::Sse3Ctl => Some(true),
            _ => None,
        }
    }

    fn config(self, n: usize) -> ModelConfig {
        let base = ModelConfig::for_size(n as u64);
        let base = match self {
            Method::RawCmNoWord => {
                let keep: Vec<usize> = (0..base.specs.len().saturating_sub(2)).collect();
                base.ablated(&keep)
            }
            _ => base,
        };
        let base = match self {
            m if m.on_phase4_parent() => base.with_column(false),
            Method::Column
            | Method::ColumnCase
            | Method::ColumnCaseMark
            | Method::ColumnWordToken
            | Method::ColumnWordTokenReverse
            | Method::ColumnWordToken2
            | Method::LongMatch6
            | Method::LongMatch8
            | Method::LongMatch12
            | Method::LongMatch16
            | Method::LongMatch24
            | Method::SparseMatch4g1
            | Method::SparseMatch6g1
            | Method::SparseMatch6g2
            | Method::RepState1
            | Method::RepState2
            | Method::RepState3
            | Method::RepState4
            | Method::MatchByte
            | Method::MatchByteConst
            | Method::Phase4
            | Method::DistMatch
            | Method::WordClass
            | Method::WordClassConst
            | Method::ColumnWordTokenPhrase
            | Method::ColumnWordTokenPhraseFreq
            | Method::ColumnWordTokenFront
            | Method::ColumnWordTokenAffix
            | Method::StateMap
            | Method::StateMapCtl
            | Method::StateMapRep
            | Method::Sse3
            | Method::Sse3Ctl => base.with_column(false),
            Method::ColumnShuffled => base.with_column(true),
            Method::ColumnNoLine => base.with_column_kind(crate::context::CtxKind::ColumnNoLine),
            _ => base,
        };
        let base = match self.match2_min() {
            Some(m) => base.with_match2(m),
            None => base,
        };
        let base = match self.sparse_tier() {
            Some((min, gap)) => base.with_match_tier(min, gap),
            None => base,
        };
        let base = base.with_rep_offsets(self.rep_offsets());
        let base = match self.match_byte_kind() {
            Some(ctl) => base.with_match_byte(ctl),
            None => base,
        };
        let base = if self.dist_match() {
            base.with_dist_match()
        } else {
            base
        };
        let base = match self.word_class_kind() {
            Some(ctl) => base.with_word_class(ctl),
            None => base,
        };
        let base = match self.state_map_kind() {
            Some(false) => base.with_state_orders(&[2, 4, 6]),
            Some(true) => base.with_orders(&[2, 4, 6]),
            None => base,
        };
        let base = if self.state_map_rep() {
            base.with_replaced_state_orders(&[2, 4, 6])
        } else {
            base
        };
        let base = match self.sse3_kind() {
            Some(false) => base.with_sse3(),
            Some(true) => base.with_sse3_ctl(),
            None => base,
        };
        // Phase 6 parent: the accepted config plus the extra SSE stage (the
        // state-map experts were rejected at enwik9 scale). The remaining
        // Phase-6 mechanisms are measured on top of this.
        let base = if self.base6() {
            match self {
                Method::Isse => base.with_isse(),
                Method::IsseCtl => base.with_isse_ctl(),
                _ => base.with_sse3(),
            }
        } else {
            base
        };
        // Phase 6.5: sparse (gapped) context experts. `-ctl` adds a direct order-4
        // expert instead (width control); `-rep` swaps the direct order-4 expert
        // for a sparse one at identical mixer width (representation control).
        let base = match self {
            Method::Sparse4g1 => base.with_sparse(4, 1),
            Method::Sparse4g1Ctl => base.with_orders(&[4]),
            Method::Sparse4g1Rep => base.with_replaced_sparse(4, 1),
            _ => base,
        };
        // Phase 6.2/6.6: indirect context models.
        let base = match self {
            Method::Icm => base.with_indirect(2, 2, false),
            Method::IcmCtl => base.with_indirect_ctl(2, 2),
            Method::IndirectChain => base.with_indirect(2, 2, true),
            Method::IndirectDeep => base.with_indirect(5, 2, false),
            _ => base,
        };
        // Phase 6.7: checksum-verified context slots.
        let base = match self {
            Method::Collision => base.with_collision(1),
            Method::CollisionCtl => base.with_collision(2),
            _ => base,
        };
        // Phase 6.10: stem-folded word model (and its raw-word width control).
        let base = match self {
            Method::StemModel => base.with_stem_model(false),
            Method::StemModelCtl => base.with_stem_model(true),
            _ => base,
        };
        // Phase 6.8: bounded PPM-C byte model; `-ctl` keeps only order 1 so the
        // multi-order escape/backoff is isolated.
        let base = match self {
            Method::Ppm => base.with_ppm(4),
            Method::PpmCtl => base.with_ppm(1),
            // Phase 6.9: the direct orders that PPM subsumes are pruned.
            Method::Spine => base.with_ppm(4).without_orders(&[4, 8, 12, 16]),
            _ => base,
        };
        // Phase 8: the learned residual corrector (frozen weights; the control
        // permutes them).
        let base = match self {
            Method::Residual => base.with_residual(false),
            // Phase 10.4: the representation variants are the accepted composite
            // plus a different token id assignment, so they keep the corrector.
            Method::ResidualMtf | Method::ResidualMoveToSecond => base.with_residual(false),
            Method::ResidualPriced => base.with_residual(false),
            Method::ResidualPriceFilter => base.with_residual(false),
            Method::ResidualAffix => base.with_residual(false),
            Method::ResidualFirstUse => base.with_residual(false),
            Method::ResidualSubword => base.with_residual(false),
            Method::ResidualCtl => base.with_residual(true),
            _ => base,
        };
        base.with_info(self.info())
    }

    /// Phase 6.9 (research): the model configuration this method builds, for
    /// expert-roster pruning measurements and memory projection.
    pub fn config_for(self, n: usize) -> ModelConfig {
        self.config(n)
    }
}

// Phase 5: grammar induction/serialization modes.
#[cfg(feature = "grammar")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrammarMode {
    Verbatim,
    Mtf,
    Mr,
    Rrank,
    FirstUse,
    OneShot,
}

// --- transform plumbing, feature-gated so binary cost is measurable ---------

#[cfg(feature = "struct-hoist")]
fn maybe_hoist(method: Method, input: &[u8]) -> Vec<u8> {
    if method.hoists() {
        crate::transform::encode(input)
    } else {
        input.to_vec()
    }
}

#[cfg(not(feature = "struct-hoist"))]
fn maybe_hoist(_method: Method, input: &[u8]) -> Vec<u8> {
    input.to_vec()
}

#[cfg(feature = "struct-hoist")]
fn maybe_unhoist(method: Method, data: Vec<u8>) -> Vec<u8> {
    if method.hoists() {
        crate::transform::decode(&data)
    } else {
        data
    }
}

#[cfg(not(feature = "struct-hoist"))]
fn maybe_unhoist(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

#[cfg(feature = "alphabet-perm")]
fn maybe_perm(method: Method, data: Vec<u8>) -> (Vec<u8>, Option<[u8; PERM_LEN]>) {
    match method.perm() {
        PermKind::None => (data, None),
        PermKind::Freq => {
            let p = crate::transform::frequency_perm(&data);
            let e = crate::transform::apply_perm(&data, &p);
            (e, Some(p))
        }
        PermKind::Random => {
            let p = crate::transform::random_perm(0x5EED_5EED);
            let e = crate::transform::apply_perm(&data, &p);
            (e, Some(p))
        }
    }
}

#[cfg(not(feature = "alphabet-perm"))]
fn maybe_perm(_method: Method, data: Vec<u8>) -> (Vec<u8>, Option<[u8; PERM_LEN]>) {
    (data, None)
}

#[cfg(feature = "alphabet-perm")]
fn maybe_unperm(data: Vec<u8>, perm: Option<&[u8; PERM_LEN]>) -> Vec<u8> {
    match perm {
        Some(p) => {
            let inv = crate::transform::invert_perm(p);
            crate::transform::apply_perm(&data, &inv)
        }
        None => data,
    }
}

#[cfg(not(feature = "alphabet-perm"))]
fn maybe_unperm(data: Vec<u8>, _perm: Option<&[u8; PERM_LEN]>) -> Vec<u8> {
    data
}

// A1.2 case factorization runs *after* structural hoisting: hoisting replaces
// structural strings with codes in 0x01..=0x1F, and case markers reuse the low
// bytes 0x00..=0x03. Running case second is what keeps them from colliding: the
// only hoist codes case must escape are the first few dictionary codes, which
// are rare, while every case marker costs exactly one byte. It is a bijection
// with a universal escape, so `maybe_uncase` inverts it exactly.
#[cfg(feature = "case-model")]
fn maybe_case(method: Method, data: Vec<u8>) -> Vec<u8> {
    match method.case_kind() {
        CaseKind::None => data,
        CaseKind::Merge => crate::transform::case_encode(&data),
        CaseKind::MarkOnly => crate::transform::case_encode_markonly(&data),
    }
}

#[cfg(not(feature = "case-model"))]
fn maybe_case(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

#[cfg(feature = "case-model")]
fn maybe_uncase(method: Method, data: Vec<u8>) -> Vec<u8> {
    match method.case_kind() {
        CaseKind::None => data,
        CaseKind::Merge => crate::transform::case_decode(&data),
        CaseKind::MarkOnly => crate::transform::case_decode_markonly(&data),
    }
}

#[cfg(not(feature = "case-model"))]
fn maybe_uncase(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

// A1.1/A26 dynamic word vocabulary. Applied after hoisting and case so it sees
// normalized text; its `0x00` prefix is escaped, so it composes with the other
// byte transforms exactly.
#[cfg(any(feature = "word-token", feature = "word-token2"))]
fn maybe_token(method: Method, data: Vec<u8>, tune: u8) -> Vec<u8> {
    // Phase 10: the model-priced vocabulary. The pricing pass encodes this very
    // stream (the tokenizer's input) once, so the cost is one extra encode — paid
    // only by the experimental method, and never by the scored configuration. The
    // chosen vocabulary travels in the stream's own header, so the decoder is
    // unchanged and cannot be made incorrect by it.
    #[cfg(feature = "vocab-price")]
    if method.priced_vocab() {
        match method.vocab_policy() {
            VocabPolicy::Filter => {
                let (vocab, blocked) = priced_filter(&data, method, tune);
                return crate::transform::word_token_encode_vocab_filtered(
                    &data,
                    &vocab,
                    &blocked,
                    crate::transform::IdMode::Static,
                );
            }
            VocabPolicy::Rerank => {
                let vocab = priced_vocab(&data, method, tune);
                return crate::transform::word_token_encode_vocab(
                    &data,
                    &vocab,
                    crate::transform::IdMode::Static,
                );
            }
            VocabPolicy::Shipped => {}
        }
    }
    let _ = tune;
    match method.token_kind() {
        TokenKind::None => data,
        TokenKind::Words => crate::transform::word_token_encode(&data, false),
        TokenKind::Reverse => crate::transform::word_token_encode(&data, true),
        TokenKind::V2 => crate::transform::word_token2_encode(&data),
        TokenKind::Phrase => crate::transform::word_token_phrase_encode(&data, true),
        TokenKind::PhraseFreq => crate::transform::word_token_phrase_encode(&data, false),
        TokenKind::Front => crate::transform::word_token_front_encode(&data, true),
        TokenKind::Affix => crate::transform::word_token_affix_encode(&data, true),
        TokenKind::FirstUse => {
            let vocab = crate::transform::build_word_vocab(&data, true);
            crate::transform::word_token_encode_firstuse(&data, &vocab)
        }
        TokenKind::Subword => {
            let vocab = crate::transform::build_word_vocab(&data, true);
            crate::transform::word_token_encode_subword(&data, &vocab)
        }
        TokenKind::Mtf => {
            crate::transform::word_token_encode_mode(&data, true, crate::transform::IdMode::Mtf)
        }
        TokenKind::MoveToSecond => crate::transform::word_token_encode_mode(
            &data,
            true,
            crate::transform::IdMode::MoveToSecond,
        ),
    }
}

#[cfg(not(any(feature = "word-token", feature = "word-token2")))]
fn maybe_token(_method: Method, data: Vec<u8>, _tune: u8) -> Vec<u8> {
    data
}

#[cfg(any(feature = "word-token", feature = "word-token2"))]
fn maybe_untoken(method: Method, data: Vec<u8>) -> Vec<u8> {
    match method.token_kind() {
        TokenKind::None => data,
        TokenKind::Words | TokenKind::Reverse => crate::transform::word_token_decode(&data),
        TokenKind::V2 => crate::transform::word_token2_decode(&data),
        TokenKind::Phrase | TokenKind::PhraseFreq => crate::transform::word_token_decode(&data),
        TokenKind::Front => crate::transform::word_token_front_decode(&data),
        TokenKind::Affix => crate::transform::word_token_affix_decode(&data),
        TokenKind::FirstUse => crate::transform::word_token_decode_firstuse(&data),
        TokenKind::Subword => crate::transform::word_token_decode_subword(&data),
        TokenKind::Mtf => {
            crate::transform::word_token_decode_mode(&data, crate::transform::IdMode::Mtf)
        }
        TokenKind::MoveToSecond => {
            crate::transform::word_token_decode_mode(&data, crate::transform::IdMode::MoveToSecond)
        }
    }
}

#[cfg(not(any(feature = "word-token", feature = "word-token2")))]
fn maybe_untoken(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

// Phase 4.6: stem transform, applied first (it is standalone for the screen).
#[cfg(feature = "stem")]
fn maybe_stem(method: Method, data: Vec<u8>) -> Vec<u8> {
    if method.stems() {
        crate::transform::stem_encode(&data)
    } else {
        data
    }
}

#[cfg(not(feature = "stem"))]
fn maybe_stem(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

#[cfg(feature = "stem")]
fn maybe_unstem(method: Method, data: Vec<u8>) -> Vec<u8> {
    if method.stems() {
        crate::transform::stem_decode(&data)
    } else {
        data
    }
}

#[cfg(not(feature = "stem"))]
fn maybe_unstem(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

// Phase 5: grammar transform, applied first (standalone for the screen).
#[cfg(feature = "grammar")]
const GRAMMAR_MAX_RULES: usize = 256;
#[cfg(feature = "grammar")]
const GRAMMAR_MIN_COUNT: u32 = 8;

#[cfg(feature = "grammar")]
fn maybe_grammar(method: Method, data: Vec<u8>) -> Vec<u8> {
    match method.grammar_mode() {
        Some(GrammarMode::Verbatim) => {
            crate::grammar::encode_ex(&data, GRAMMAR_MAX_RULES, GRAMMAR_MIN_COUNT, false, false)
        }
        Some(GrammarMode::Mtf) => {
            crate::grammar::encode_ex(&data, GRAMMAR_MAX_RULES, GRAMMAR_MIN_COUNT, true, false)
        }
        Some(GrammarMode::Mr) => {
            crate::grammar::encode_ex(&data, GRAMMAR_MAX_RULES, GRAMMAR_MIN_COUNT, false, true)
        }
        Some(GrammarMode::Rrank) => {
            crate::grammar::encode_rrank(&data, GRAMMAR_MAX_RULES, GRAMMAR_MIN_COUNT)
        }
        Some(GrammarMode::FirstUse) => {
            crate::grammar::encode_firstuse(&data, GRAMMAR_MAX_RULES, GRAMMAR_MIN_COUNT)
        }
        Some(GrammarMode::OneShot) => {
            crate::grammar::encode_oneshot(&data, GRAMMAR_MAX_RULES, GRAMMAR_MIN_COUNT)
        }
        None => data,
    }
}

#[cfg(not(feature = "grammar"))]
fn maybe_grammar(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

#[cfg(feature = "grammar")]
fn maybe_ungrammar(method: Method, data: Vec<u8>) -> Vec<u8> {
    match method.grammar_mode() {
        Some(GrammarMode::FirstUse) => crate::grammar::decode_firstuse(&data),
        Some(_) => crate::grammar::decode(&data),
        None => data,
    }
}

#[cfg(not(feature = "grammar"))]
fn maybe_ungrammar(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

#[cfg(feature = "grammar")]
fn maybe_lzbe(method: Method, data: Vec<u8>) -> Vec<u8> {
    if method.lzbe() {
        crate::grammar::lzbe_encode(&data, 256, 64)
    } else {
        data
    }
}

#[cfg(not(feature = "grammar"))]
fn maybe_lzbe(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

#[cfg(feature = "grammar")]
fn maybe_unlzbe(method: Method, data: Vec<u8>) -> Vec<u8> {
    if method.lzbe() {
        crate::grammar::lzbe_decode(&data)
    } else {
        data
    }
}

#[cfg(not(feature = "grammar"))]
fn maybe_unlzbe(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

/// Phase 7 inverse: restore the original page order. `method.reorders()` is true
/// only for a method the *encoder* actually reordered, so a corpus that failed
/// the precondition (and was encoded under the parent method) is never sorted.
#[cfg(feature = "reorder")]
fn maybe_unreorder(method: Method, data: Vec<u8>) -> Vec<u8> {
    if method.reorders() {
        crate::reorder::restore(&data)
    } else {
        data
    }
}

#[cfg(not(feature = "reorder"))]
fn maybe_unreorder(_method: Method, data: Vec<u8>) -> Vec<u8> {
    data
}

// --- codec ------------------------------------------------------------------

/// The method and tune of the currently accepted candidate. `encode` uses these
/// so the production path (bench, compress, SFX) always reflects the best
/// measured configuration. Optimization-A experiments override both explicitly.
///
/// A1.1/A26: corpus-derived word tokenization with reverse-frequency ids on top
/// of structural hoisting and the column expert, extended by Phase 4's match
/// family: a long-distance tier (4.1), a sparse/gapped tier (4.2) and the
/// matched-literal expert (4.4). Adopted at enwik9: archive 176,204,762 (1.4096
/// bpc vs 1.4406), DeltaS -3,869,716 at a measured 5,200 B executable cost.
///
/// Phase 6 (6.4): the extra order-2 SSE stage (`sse-3`) is adopted on top of the
/// Phase-4 composite. Adopted at enwik9: archive 174,533,527 (1.3963 bpc vs
/// 1.4096), DeltaS -1,670,979 at a measured 256 B executable cost; the control
/// (an uncorrelated key) is only -305,341.
///
/// Phase 7 (7.8): the article-layout compiler is adopted on top of `sse-3`. The
/// encoder orders `<page>` blocks by their category set, then template set, then
/// title; the decoder restores the original order by a stable sort on the
/// embedded ascending page id (zero permutation bytes). Adopted at enwik9:
/// archive 170,063,733 (1.3605 bpc vs 1.3963), DeltaS -4,469,794 at a measured
/// 24,208 B executable cost; the identity control is exactly 0 and the shuffle
/// control is +25,519 at enwik7.
///
/// Phase 8 (8.3): the learned residual corrector is adopted on top of the
/// article layout. A 120-byte quantized MLP corrects the classical chain's logit
/// from a 12-dim classical feature vector. Adopted at enwik9: archive
/// 169,642,087 (1.3571 bpc vs 1.3605), DeltaS -421,646 at a measured 7,848 B
/// executable cost (weights embedded); the permuted-weight control is +2,201,020.
pub const ACCEPTED_METHOD: Method = Method::Residual;
/// Phase 9: the optimal mixer learning rate is a function of the *predictor*, and
/// Phases 6–8 changed the predictor. A20 tuned LR 24 against the Phase-4 model on
/// the smaller rungs; re-searched on the authority corpus with the mature model,
/// every step down the ladder wins by more:
///
/// ```text
/// tune 7  LR 24   169,642,087   (the previously accepted value)
/// tune 6  LR 20   169,484,029   -158,058
/// tune 5  LR 16   169,282,339   -359,748   <- adopted
/// ```
///
/// Each figure is a full `eval` gate on enwik9 with exact reconstruction. The
/// knob costs **zero** executable bytes: the ladder already existed and the chosen
/// point has the APM axis off, so it decodes identically with that rejected axis
/// compiled out.
pub const ACCEPTED_TUNE: u8 = 5;

/// Compress `input` into an archive payload using the accepted configuration.
pub fn encode(input: &[u8]) -> Vec<u8> {
    encode_tuned(input, ACCEPTED_METHOD, ACCEPTED_TUNE)
}

/// Compress with an explicit method (used by ablation and Phase-A runs).
pub fn encode_with(input: &[u8], method: Method) -> Vec<u8> {
    encode_tuned(input, method, 0)
}

/// Phase 7: apply the encoder-side article ordering. Returns the (possibly
/// reordered) stream and the *effective* method: when the corpus does not
/// satisfy the free-restoration precondition the method is downgraded to its
/// parent, so the decoder never attempts a sort it cannot invert.
#[cfg(feature = "reorder")]
fn prepare_reorder(input: &[u8], method: Method) -> (Vec<u8>, Method) {
    match method.reorder_kind() {
        Some(order) => match crate::reorder::encode(input, order) {
            Some(v) => (v, method),
            None => (input.to_vec(), method.reorder_parent()),
        },
        None => (input.to_vec(), method),
    }
}

#[cfg(not(feature = "reorder"))]
fn prepare_reorder(input: &[u8], method: Method) -> (Vec<u8>, Method) {
    (input.to_vec(), method)
}

/// Compress with an explicit method and optimizer tuning variant (A20).
/// `tune` selects a learning-rate/update variant at *runtime*, so it costs no
/// additional executable bytes; the value is stored in the header and the
/// decoder reconstructs the identical model.
pub fn encode_tuned(input: &[u8], method: Method, tune: u8) -> Vec<u8> {
    // Phase 7: the article-layout compiler runs outermost, on the raw corpus.
    // The decoder's inverse is the very last step of `decode`.
    let (input2, method) = prepare_reorder(input, method);
    let lzbed = maybe_lzbe(method, input2);
    let grammared = maybe_grammar(method, lzbed);
    let stemmed = maybe_stem(method, grammared);
    let hoisted = maybe_hoist(method, &stemmed);
    let cased = maybe_case(method, hoisted);
    let tokened = maybe_token(method, cased, tune);
    let (data, perm) = maybe_perm(method, tokened);
    let n = data.len();

    let mut out = Vec::with_capacity(HEADER_LEN + PERM_LEN + n / 2);
    out.extend_from_slice(MAGIC);
    out.push(method as u8);
    out.push(tune);
    out.extend_from_slice(&(n as u64).to_le_bytes());
    if let Some(p) = &perm {
        out.extend_from_slice(p);
    }

    let cfg = method.config(n).with_tune(tune);
    let mut cm = Cm::new(&cfg, n);
    let mut enc = RangeEncoder::with_capacity(n / 2 + 64);
    #[cfg(feature = "progress")]
    let mut prog = crate::progress::Progress::new("encode", n as u64);

    for (i, &byte) in data.iter().enumerate() {
        // Protect the machine: a long run re-checks available memory as it codes.
        // The guard is armed only by the research driver, so this is a no-op on
        // the judged path (one relaxed atomic load per megabyte). The same point
        // reports progress, which is how a run shows it is alive rather than
        // appearing to hang for half an hour.
        if i & (crate::memory::RUNTIME_CHECK_INTERVAL - 1) == 0 {
            crate::memory::enforce_runtime_floor();
            #[cfg(feature = "progress")]
            prog.tick(i as u64);
        }
        let mut mask = 0x80u32;
        while mask != 0 {
            let bit = if (byte as u32) & mask != 0 { 1 } else { 0 };
            let p = cm.predict();
            enc.encode(bit, p);
            cm.update(bit);
            mask >>= 1;
        }
    }
    #[cfg(feature = "progress")]
    prog.finish();
    out.extend_from_slice(&enc.finish());
    out
}

/// Phase 6.9 (research only): compress with an explicit expert roster while
/// keeping the method's transforms. The header still records `method`, so the
/// output is a *size* measurement for pruning — not a decodable archive. Any
/// roster that survives pruning is re-verified through a real [`Method`].
#[cfg(not(feature = "submission"))]
pub fn encode_specs(
    input: &[u8],
    method: Method,
    tune: u8,
    specs: &[crate::context::ModelSpec],
) -> Vec<u8> {
    encode_specs_layout(input, method, tune, specs, &[])
}

/// T1 (research only): as [`encode_specs`], but also overriding each expert's
/// storage layout. Same contract — a size *and* time measurement against the
/// real transform pipeline and the real predictor, not a decodable archive.
/// `layouts` may be empty (every expert [`crate::context::Layout::Hashed`]) or
/// parallel to `specs`.
#[cfg(not(feature = "submission"))]
pub fn encode_specs_layout(
    input: &[u8],
    method: Method,
    tune: u8,
    specs: &[crate::context::ModelSpec],
    layouts: &[crate::context::Layout],
) -> Vec<u8> {
    // Phase 7: same outermost reorder as `encode_tuned`, so pruning measurements
    // apply to the reordered representation.
    let (input2, method) = prepare_reorder(input, method);
    let lzbed = maybe_lzbe(method, input2);
    let grammared = maybe_grammar(method, lzbed);
    let stemmed = maybe_stem(method, grammared);
    let hoisted = maybe_hoist(method, &stemmed);
    let cased = maybe_case(method, hoisted);
    let tokened = maybe_token(method, cased, tune);
    let (data, perm) = maybe_perm(method, tokened);
    let n = data.len();

    let mut out = Vec::with_capacity(HEADER_LEN + PERM_LEN + n / 2);
    out.extend_from_slice(MAGIC);
    out.push(method as u8);
    out.push(tune);
    out.extend_from_slice(&(n as u64).to_le_bytes());
    if let Some(p) = &perm {
        out.extend_from_slice(p);
    }

    let mut cfg = method.config(n).with_tune(tune);
    cfg.specs = specs.to_vec();
    cfg.layouts = layouts.to_vec();
    let mut cm = Cm::new(&cfg, n);
    let mut enc = RangeEncoder::with_capacity(n / 2 + 64);
    for (i, &byte) in data.iter().enumerate() {
        if i & (crate::memory::RUNTIME_CHECK_INTERVAL - 1) == 0 {
            crate::memory::enforce_runtime_floor();
        }
        let mut mask = 0x80u32;
        while mask != 0 {
            let bit = if (byte as u32) & mask != 0 { 1 } else { 0 };
            let p = cm.predict();
            enc.encode(bit, p);
            cm.update(bit);
            mask >>= 1;
        }
    }
    out.extend_from_slice(&enc.finish());
    out
}

/// Phase 8 (research only): the byte stream the predictor actually codes for a
/// method — the transforms applied, without coding. Used by the residual trainer
/// so training sees exactly the distribution inference sees.
#[cfg(not(feature = "submission"))]
pub fn transformed_stream(input: &[u8], method: Method, tune: u8) -> Vec<u8> {
    let _ = tune;
    let (input2, method) = prepare_reorder(input, method);
    let lzbed = maybe_lzbe(method, input2);
    let grammared = maybe_grammar(method, lzbed);
    let stemmed = maybe_stem(method, grammared);
    let hoisted = maybe_hoist(method, &stemmed);
    let cased = maybe_case(method, hoisted);
    let tokened = maybe_token(method, cased, tune);
    let (data, _perm) = maybe_perm(method, tokened);
    data
}

/// Phase 10 (research only): one candidate word's *measured* economics.
#[cfg(feature = "vocab-price")]
#[derive(Debug, Clone)]
pub struct WordPrice {
    pub word: Vec<u8>,
    pub count: u64,
    /// Bits the model actually charged for the word's literal occurrences in the
    /// untokened stream. This is the quantity the shipped heuristic cannot see:
    /// it compares against `len` raw bytes, so a word the model already predicts
    /// almost for free looks exactly as valuable as one it does not.
    pub literal_bits: f64,
    /// What `count` two-byte tokens would cost at the stream's measured average
    /// bits/byte.
    pub token_bits: f64,
    /// The one-off dictionary definition, at the same rate.
    pub def_bits: f64,
}

#[cfg(feature = "vocab-price")]
impl WordPrice {
    /// Bits saved by representing this word as a token instead of literally.
    /// Negative means the substitution would *cost* bytes.
    pub fn gain_bits(&self) -> f64 {
        self.literal_bits - self.token_bits - self.def_bits
    }
}

/// Phase 10 (research only): price every candidate word in `stream` by what the
/// real model charges for it.
///
/// `stream` must be the tokenizer's **input** (the post-hoist, post-case stream).
/// Returns `(prices, average bits/byte, stream length)`.
#[cfg(feature = "vocab-price")]
pub fn price_stream(stream: &[u8], method: Method, tune: u8) -> (Vec<WordPrice>, f64, usize) {
    use std::collections::HashMap;
    let n = stream.len();
    let cfg = method.config(n).with_tune(tune);
    let mut cm = Cm::new(&cfg, n);
    let mut enc = RangeEncoder::with_capacity(n / 2 + 64);

    let mut acc: HashMap<Vec<u8>, (u64, f64)> = HashMap::new();
    let mut word: Vec<u8> = Vec::new();
    // `wstart` is the cumulative cost *before* the word's first byte, and `cum`
    // the cumulative cost after the previous byte, so the difference is exactly
    // the model's price for the word's own bytes.
    let mut wstart = 0.0f64;
    let mut cum = 0.0f64;
    for &byte in stream.iter() {
        let mut mask = 0x80u32;
        while mask != 0 {
            let bit = if (byte as u32) & mask != 0 { 1 } else { 0 };
            let p = cm.predict();
            enc.encode(bit, p);
            cm.update(bit);
            mask >>= 1;
        }
        let now = enc.cost_bits();
        if crate::transform::is_word_byte_at(byte) {
            if word.is_empty() {
                wstart = cum;
            }
            word.push(byte);
        } else if !word.is_empty() {
            let e = acc.entry(std::mem::take(&mut word)).or_insert((0, 0.0));
            e.0 += 1;
            e.1 += cum - wstart;
        }
        cum = now;
    }
    if !word.is_empty() {
        let e = acc.entry(word).or_insert((0, 0.0));
        e.0 += 1;
        e.1 += cum - wstart;
    }

    let avg_bpb = if n > 0 { cum / n as f64 } else { 0.0 };
    let prices = acc
        .into_iter()
        .map(|(w, (count, literal_bits))| {
            let len = w.len() as f64;
            WordPrice {
                word: w,
                count,
                literal_bits,
                token_bits: count as f64 * 2.0 * avg_bpb,
                def_bits: (len + 1.0) * avg_bpb,
            }
        })
        .collect();
    (prices, avg_bpb, n)
}

/// Phase 10: the vocabulary a **model-priced** membership rule selects from
/// `stream`, in the id order the encoder will store.
///
/// Ranked by measured gain (descending, ties by word so the result is
/// deterministic), keeping only entries that actually save bits. The list is then
/// reversed to mirror the accepted configuration's id convention — the id *order*
/// was measured in A1.1 and is inherited here rather than re-searched, so this
/// experiment changes membership only and leaves the identity axis alone.
#[cfg(feature = "vocab-price")]
pub fn priced_vocab(stream: &[u8], method: Method, tune: u8) -> Vec<Vec<u8>> {
    let (prices, _avg, _n) = price_stream(stream, method, tune);
    let mut ranked: Vec<&WordPrice> = prices.iter().collect();
    ranked.sort_by(|a, b| {
        b.gain_bits()
            .partial_cmp(&a.gain_bits())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.word.cmp(&b.word))
    });
    let mut vocab: Vec<Vec<u8>> = ranked
        .iter()
        .filter(|p| p.gain_bits() > 0.0)
        .take(crate::transform::MAX_TOKENS)
        .map(|p| p.word.clone())
        .collect();
    vocab.reverse();
    vocab
}

/// Phase 10 (research only): price the vocabulary the shipped pipeline uses, by
/// building the tokenizer's input and handing it to [`price_stream`].
///
/// Returns `(prices, average bits/byte, stream length, shipped vocabulary)`. The
/// vocabulary is stored
/// in the archive and read back by the decoder, so *choosing* it is entirely
/// encoder-side: this pass changes no format, is invisible to `decode`, and
/// therefore cannot threaten exactness. That is what makes a repriced vocabulary
/// cheap to test where a repriced *bitstream* would not be.
#[cfg(feature = "vocab-price")]
pub fn price_vocabulary(
    input: &[u8],
    method: Method,
    tune: u8,
) -> (Vec<WordPrice>, f64, usize, Vec<Vec<u8>>) {
    let stream = untokened_stream(input, method, tune);
    let (prices, avg_bpb, n) = price_stream(&stream, method, tune);
    // The vocabulary the shipped tokenizer would build from this same stream.
    let shipped = crate::transform::build_word_vocab(&stream, true);
    (prices, avg_bpb, n, shipped)
}

/// Phase 10: a vocabulary policy.
#[cfg(feature = "vocab-price")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum VocabPolicy {
    /// The shipped count heuristic.
    Shipped,
    /// Re-rank candidate words by measured gain and take the same number of slots.
    Rerank,
    /// Keep the shipped vocabulary and ids, but stop *substituting* the words the
    /// model prices as costing more than they save.
    Filter,
}

/// Phase 10: the vocabulary the **filter** policy blocks from substitution, and
/// the shipped list whose ids must stay intact.
#[cfg(feature = "vocab-price")]
fn priced_filter(
    stream: &[u8],
    method: Method,
    tune: u8,
) -> (Vec<Vec<u8>>, std::collections::HashSet<Vec<u8>>) {
    let shipped = crate::transform::build_word_vocab(stream, true);
    let (prices, _avg, _n) = price_stream(stream, method, tune);
    let mut gain: std::collections::HashMap<&[u8], f64> = std::collections::HashMap::new();
    for p in &prices {
        gain.insert(p.word.as_slice(), p.gain_bits());
    }
    let blocked = shipped
        .iter()
        .filter(|w| gain.get(w.as_slice()).copied().unwrap_or(0.0) <= 0.0)
        .cloned()
        .collect();
    (shipped, blocked)
}

/// Phase 10 (research only): the pipeline up to (but not including) the
/// tokenizer, which is the stream the tokenizer both consumes and is priced on.
#[cfg(feature = "vocab-price")]
pub fn untokened_stream(input: &[u8], method: Method, tune: u8) -> Vec<u8> {
    let _ = tune;
    let (input2, method) = prepare_reorder(input, method);
    let lzbed = maybe_lzbe(method, input2);
    let grammared = maybe_grammar(method, lzbed);
    let stemmed = maybe_stem(method, grammared);
    let hoisted = maybe_hoist(method, &stemmed);
    let cased = maybe_case(method, hoisted);
    let (data, _perm) = maybe_perm(method, cased);
    data
}

/// Map an archive header's method byte to a [`Method`].
///
/// Every method is decodable, so any ablation can be reproduced and every negative
/// result stays inspectable.
///
/// It was worth testing whether a *submission* build could narrow this to the two
/// ids the accepted encoder can emit — [`Method::Residual`], plus [`Method::Sse3`]
/// as the article-layout compiler's downgrade target for a corpus whose page ids
/// are not strictly ascending. Measured: narrowing it saved **minus 24 B**, i.e.
/// nothing, because fat LTO already removes the model-configuration code that no
/// reachable method reaches. The dispatch is not where a scored artifact's bytes
/// go; see `docs/RESOURCE_CLOSURE.md` section 6. The
/// experiment was removed rather than kept, since a two-method decoder is a
/// correctness risk that buys nothing.
fn method_from_id(id: u8) -> Option<Method> {
    Some(match id {
        0 => Method::RawCm,
        1 => Method::RawCmNoWord,
        2 => Method::StructHoist,
        3 => Method::AlphabetPerm,
        4 => Method::AlphabetRandom,
        5 => Method::InfoInherit,
        6 => Method::InfoUnrelated,
        7 => Method::AlphabetPermInfo,
        8 => Method::Column,
        9 => Method::ColumnShuffled,
        10 => Method::ColumnNoLine,
        11 => Method::Case,
        12 => Method::CaseMark,
        13 => Method::ColumnCase,
        14 => Method::ColumnCaseMark,
        15 => Method::WordToken,
        16 => Method::WordTokenReverse,
        17 => Method::ColumnWordToken,
        18 => Method::ColumnWordTokenReverse,
        19 => Method::WordToken2,
        20 => Method::ColumnWordToken2,
        21 => Method::LongMatch6,
        22 => Method::LongMatch8,
        23 => Method::LongMatch12,
        24 => Method::LongMatch16,
        25 => Method::LongMatch24,
        26 => Method::SparseMatch4g1,
        27 => Method::SparseMatch6g1,
        28 => Method::SparseMatch6g2,
        29 => Method::RepState1,
        30 => Method::RepState2,
        31 => Method::RepState3,
        32 => Method::RepState4,
        33 => Method::MatchByte,
        34 => Method::MatchByteConst,
        35 => Method::Phase4,
        36 => Method::DistMatch,
        37 => Method::Stem,
        38 => Method::WordClass,
        39 => Method::WordClassConst,
        40 => Method::ColumnWordTokenPhrase,
        41 => Method::ColumnWordTokenPhraseFreq,
        42 => Method::ColumnWordTokenFront,
        43 => Method::ColumnWordTokenAffix,
        44 => Method::Grammar,
        45 => Method::GrammarMtf,
        46 => Method::GrammarMr,
        47 => Method::GrammarRrank,
        48 => Method::GrammarFirstUse,
        49 => Method::GrammarOneShot,
        50 => Method::Lzbe,
        51 => Method::StateMap,
        52 => Method::StateMapCtl,
        53 => Method::StateMapRep,
        54 => Method::Sse3,
        55 => Method::Sse3Ctl,
        56 => Method::Sparse4g1,
        57 => Method::Sparse4g1Ctl,
        58 => Method::Sparse4g1Rep,
        59 => Method::Icm,
        60 => Method::IcmCtl,
        61 => Method::IndirectChain,
        62 => Method::IndirectDeep,
        63 => Method::Collision,
        64 => Method::CollisionCtl,
        65 => Method::Ppm,
        66 => Method::PpmCtl,
        67 => Method::StemModel,
        68 => Method::StemModelCtl,
        69 => Method::Isse,
        70 => Method::IsseCtl,
        71 => Method::Phase6,
        72 => Method::Spine,
        73 => Method::ReorderId,
        74 => Method::ReorderTitle,
        75 => Method::ReorderSize,
        76 => Method::ReorderStruct,
        77 => Method::ReorderMinHash,
        78 => Method::ReorderGreedy,
        79 => Method::ReorderTemplate,
        80 => Method::ReorderShuffle,
        81 => Method::ReorderCategory,
        82 => Method::ReorderTemplateKey,
        83 => Method::ReorderCategorySet,
        84 => Method::ReorderFull,
        85 => Method::ReorderFullResidual,
        86 => Method::Residual,
        87 => Method::ResidualCtl,
        88 => Method::ResidualMtf,
        89 => Method::ResidualMoveToSecond,
        90 => Method::ResidualPriced,
        91 => Method::ResidualPriceFilter,
        92 => Method::ResidualAffix,
        93 => Method::ResidualFirstUse,
        94 => Method::ResidualSubword,
        _ => return None,
    })
}

/// Decode an archive payload produced by [`encode_with`].
pub fn decode(archive: &[u8]) -> Option<Vec<u8>> {
    if archive.len() < HEADER_LEN {
        return None;
    }
    if &archive[0..4] != MAGIC {
        return None;
    }
    let method = method_from_id(archive[4])?;
    let mut len_bytes = [0u8; 8];
    len_bytes.copy_from_slice(&archive[6..14]);
    let n = u64::from_le_bytes(len_bytes);
    if n > MAX_OUTPUT {
        return None;
    }
    let n = n as usize;
    let tune = archive[5];

    let mut off = HEADER_LEN;
    let perm = if method.perm() != PermKind::None {
        if archive.len() < off + PERM_LEN {
            return None;
        }
        let mut p = [0u8; PERM_LEN];
        p.copy_from_slice(&archive[off..off + PERM_LEN]);
        off += PERM_LEN;
        Some(p)
    } else {
        None
    };
    let payload = &archive[off..];

    let cfg = method.config(n).with_tune(tune);
    let mut cm = Cm::new(&cfg, n);
    let mut dec = RangeDecoder::new(payload);
    let mut decoded = Vec::with_capacity(n);
    #[cfg(feature = "progress")]
    let mut prog = crate::progress::Progress::new("decode", n as u64);
    for i in 0..n {
        // Decode is as long as encode, so it carries the same runtime floor and
        // the same progress reporting.
        if i & (crate::memory::RUNTIME_CHECK_INTERVAL - 1) == 0 {
            crate::memory::enforce_runtime_floor();
            #[cfg(feature = "progress")]
            prog.tick(i as u64);
        }
        let mut byte = 0u32;
        for _ in 0..8 {
            let p = cm.predict();
            let bit = dec.decode(p);
            cm.update(bit);
            byte = (byte << 1) | bit;
        }
        decoded.push(byte as u8);
    }
    #[cfg(feature = "progress")]
    prog.finish();

    let unpermuted = maybe_unperm(decoded, perm.as_ref());
    let untokened = maybe_untoken(method, unpermuted);
    let uncased = maybe_uncase(method, untokened);
    let unhoisted = maybe_unhoist(method, uncased);
    let unstemmed = maybe_unstem(method, unhoisted);
    let ungrammared = maybe_ungrammar(method, unstemmed);
    let unlzbed = maybe_unlzbe(method, ungrammared);
    // Phase 7: the article-layout compiler's inverse is the final step.
    Some(maybe_unreorder(method, unlzbed))
}

/// Peek the coded length from an archive header without decoding it. Used by
/// the memory guard to decide whether a decode can start safely.
pub fn peek_len(archive: &[u8]) -> Option<u64> {
    if archive.len() < HEADER_LEN || &archive[0..4] != MAGIC {
        return None;
    }
    let mut b = [0u8; 8];
    b.copy_from_slice(&archive[6..14]);
    let n = u64::from_le_bytes(b);
    if n > MAX_OUTPUT {
        None
    } else {
        Some(n)
    }
}

/// Convenience: compress and report the score against the canonical corpus.
pub fn encode_with_report(input: &[u8]) -> (Vec<u8>, String) {
    let arch = encode(input);
    let served = crate::score::Score::new(0, arch.len() as u64);
    let report = format!(
        "input={} archive={} ratio={:.4} bits/byte(enwik9-scale)={:.4}",
        input.len(),
        arch.len(),
        input.len() as f64 / arch.len() as f64,
        served.bits_per_byte()
    );
    (arch, report)
}

/// SFX packager/reader shared format.
pub const SFX_MARKER: &[u8; 15] = b"\0ZNTROPY-SFX\0\0\0";

/// Append an archive to an executable image, producing a self-extracting file.
pub fn append_sfx(image: &mut Vec<u8>, archive: &[u8]) {
    image.extend_from_slice(SFX_MARKER);
    image.extend_from_slice(&(archive.len() as u64).to_le_bytes());
    image.extend_from_slice(archive);
}

/// Recover the appended archive. The last marker wins.
pub fn extract_sfx(image: &[u8]) -> Option<&[u8]> {
    let m = image.len().checked_sub(SFX_MARKER.len())?;
    let mut i = m;
    loop {
        if &image[i..i + SFX_MARKER.len()] == SFX_MARKER {
            let len_off = i + SFX_MARKER.len();
            if len_off + 8 > image.len() {
                return None;
            }
            let mut lb = [0u8; 8];
            lb.copy_from_slice(&image[len_off..len_off + 8]);
            let len = u64::from_le_bytes(lb) as usize;
            let start = len_off + 8;
            if start + len > image.len() {
                return None;
            }
            return Some(&image[start..start + len]);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_well_formed() {
        // A page corpus with ascending ids satisfies the Phase-7 precondition, so
        // the accepted method is written verbatim (a non-page input is correctly
        // downgraded to the reorder parent, which is checked separately below).
        let data = b"  <page>\n    <title>A</title>\n    <id>1</id>\n  </page>\n  <page>\n    <title>B</title>\n    <id>2</id>\n  </page>\n".to_vec();
        let arch = encode(&data);
        assert_eq!(&arch[0..4], MAGIC);
        // `encode` uses the accepted method/tune.
        assert_eq!(arch[4], ACCEPTED_METHOD as u8);
        assert_eq!(arch[5], ACCEPTED_TUNE);
        // The length field is the length of the *transformed* stream, which the
        // inverse transforms map back to the input. It is therefore >= the input
        // length, not equal to it (a token/hoist transform can lengthen or
        // shorten it). The binding invariant is that decode is exact.
        let coded_len = u64::from_le_bytes(arch[6..14].try_into().unwrap());
        assert!(
            coded_len >= 5,
            "coded length {coded_len} does not cover the input"
        );
        assert_eq!(decode(&arch).unwrap(), data);
    }

    #[test]
    fn tuning_variants_roundtrip() {
        // The Phase-9 search space is the whole `tune` byte. Every point must
        // decode exactly, which is what makes search harmless to the decoder:
        // the knob is a header byte both sides apply identically.
        let data = b"the quick brown fox jumps over the lazy dog".repeat(200);
        for tune in 0u16..256 {
            let arch = encode_tuned(&data, Method::StructHoist, tune as u8);
            assert_eq!(decode(&arch).unwrap(), data, "tune={tune}");
        }
        // The accepted configuration too, at a spread of tunes.
        for tune in [0u8, 7, 16, 63, 128, 200, 255] {
            let arch = encode_tuned(&data, ACCEPTED_METHOD, tune);
            assert_eq!(decode(&arch).unwrap(), data, "accepted tune={tune}");
        }
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(decode(b"").is_none());
        assert!(decode(b"XXXX\x00\x00\x00\x00\x00\x00\x00\x00\x00").is_none());
        let mut a = Vec::new();
        a.extend_from_slice(MAGIC);
        a.push(0);
        a.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode(&a).is_none());
    }

    #[test]
    fn every_method_roundtrips_exactly() {
        let data = b"<page><title>Zentropy</title> the quick brown fox 123 &amp;\n"
            .repeat(60)
            .to_vec();
        for m in Method::ALL {
            let arch = encode_with(&data, m);
            assert_eq!(decode(&arch).unwrap(), data, "method {}", m.name());
        }
    }

    #[test]
    fn every_method_roundtrips_binary() {
        let data: Vec<u8> = (0..=255u8).cycle().take(10_000).collect();
        for m in Method::ALL {
            let arch = encode_with(&data, m);
            assert_eq!(decode(&arch).unwrap(), data, "method {}", m.name());
        }
    }

    /// Build a synthetic page corpus whose ids are strictly ascending and whose
    /// articles fall into a few vocabulary clusters, then check that a reorder
    /// method (a) restores exactly and (b) actually changed the order.
    #[cfg(feature = "reorder")]
    fn page_corpus() -> Vec<u8> {
        let mut v = b"<mediawiki>\n".to_vec();
        let topics = [
            ("Alpha", "alpha alpha star galaxy star"),
            ("Beta", "beta beta river water river"),
            ("Gamma", "alpha galaxy star alpha star"),
            ("Delta", "river beta water river water"),
            ("Epsilon", "alpha alpha star galaxy"),
            ("Zeta", "beta river water beta water"),
        ];
        for (id, (title, body)) in topics.iter().enumerate() {
            v.extend_from_slice(
                format!(
                    "  <page>\n    <title>{title}</title>\n    <id>{id}</id>\n    <revision>\n      <text>{body}</text>\n    </revision>\n  </page>\n"
                )
                .as_bytes(),
            );
        }
        v.extend_from_slice(b"</mediawiki>\n");
        v
    }

    /// T2: the table-size scale carried in the high nibble of `tune` must scale
    /// the model, must be derived identically by encoder and decoder (it is: both
    /// read the archive's own `tune` byte), and must round-trip exactly at every
    /// scale. A knob that changed the tables without the decoder knowing would
    /// corrupt reconstruction silently — that is the failure this test exists to
    /// make impossible.
    #[cfg(feature = "tune-table")]
    #[test]
    fn tune_table_scale_roundtrips() {
        let data = b"<page>\n  <title>Alpha</title>\n  <id>1</id>\n  <revision>\n    <text>alpha star galaxy alpha star</text>\n  </revision>\n</page>\n".to_vec();
        let base = Method::StructHoist.config_for(data.len()).with_tune(5);
        for scale in 0u8..=3 {
            let tune = 5u8 | (scale << 4);
            let cfg = Method::StructHoist.config_for(data.len()).with_tune(tune);
            // Scale 0 must be identical to no scaling at all...
            if scale == 0 {
                assert_eq!(cfg.memory_bytes(), base.memory_bytes());
            } else {
                // ...and each step must actually grow the direct-expert tables.
                assert!(
                    cfg.memory_bytes() > base.memory_bytes(),
                    "scale {scale} did not grow the model"
                );
            }
            let arch = encode_tuned(&data, Method::StructHoist, tune);
            assert_eq!(
                decode(&arch).unwrap(),
                data,
                "tune={tune} did not round-trip"
            );
        }
    }

    /// Phase 10.4: the recency-id variants must reconstruct exactly through the
    /// real archive path, and must produce *different* archives from the parent —
    /// otherwise the experiment has nothing to attribute a delta to.
    ///
    /// The two halves matter equally. Exactness is the constitutional gate;
    /// "different" is what makes the measurement meaningful rather than a long
    /// way of re-measuring the parent.
    #[cfg(feature = "id-order")]
    #[test]
    fn id_order_variants_roundtrip_and_differ() {
        // Self-contained data (not the shared page corpus, which is gated on
        // `reorder`): this test must run in a bare `--features id-order` build.
        //
        // The page ids are **strictly ascending**, which is the article-layout
        // compiler's free-restoration precondition. Without it the encoder
        // downgrades the method (to `Sse3`) and both variants then code the
        // parent's representation — producing identical archives and silently
        // testing nothing. That is not a hypothetical: the first version of this
        // test used a repeated 1,2,1,2 pattern and failed exactly that way.
        let mut data = Vec::new();
        data.extend_from_slice(b"<mediawiki>\n");
        for id in 0u32..20 {
            data.extend_from_slice(
                format!(
                    "  <page>\n    <title>Alpha{id}</title>\n    <id>{id}</id>\n    <revision>\n      <text>alpha alpha star galaxy compression compression</text>\n    </revision>\n  </page>\n"
                )
                .as_bytes(),
            );
        }
        data.extend_from_slice(b"</mediawiki>\n");
        let base = encode_tuned(&data, Method::Residual, 5);
        assert_eq!(decode(&base).unwrap(), data);
        assert_eq!(base[4], Method::Residual as u8, "parent was downgraded");
        for m in [Method::ResidualMtf, Method::ResidualMoveToSecond] {
            for tune in [0u8, 5, 37, 255] {
                // Every tune, because the id list is decoder-side state that a
                // tune perturbation must not desynchronise.
                let arch = encode_tuned(&data, m, tune);
                assert_eq!(decode(&arch).unwrap(), data, "{m:?} tune={tune} not exact");
                // The precondition must still hold for the candidate, or the
                // archive was downgraded and the comparison is meaningless.
                assert_eq!(arch[4], m as u8, "{m:?} tune={tune} was downgraded");
                assert_ne!(
                    arch, base,
                    "{m:?} produced the parent archive: the representation did not change"
                );
            }
        }
    }

    /// Phase 10: the model-priced vocabulary must (a) reconstruct exactly through
    /// the ordinary decoder and (b) actually differ from the shipped vocabulary —
    /// the screening said 100 of 255 entries differ at enwik6, so a priced
    /// vocabulary that equals the shipped one means the wiring is wrong, not that
    /// the idea failed.
    #[cfg(feature = "vocab-price")]
    #[test]
    fn priced_vocabulary_roundtrips_and_differs() {
        // Strictly ascending page ids, or the article-layout precondition fails
        // and the method is downgraded (which would silently compare the parent
        // against itself).
        let mut data = Vec::new();
        data.extend_from_slice(b"<mediawiki>\n");
        for id in 0u32..30 {
            data.extend_from_slice(
                format!(
                    "  <page>\n    <title>Alpha{id}</title>\n    <id>{id}</id>\n    <revision>\n      \
                     <text>the quick brown fox jumps over the lazy dog and the fox runs \
                     through the compression compression algorithm and the galaxy star \
                     repeats itself in the archive and the river flows</text>\n    </revision>\n  </page>\n"
                )
                .as_bytes(),
            );
        }
        data.extend_from_slice(b"</mediawiki>\n");

        let priced = encode_tuned(&data, Method::ResidualPriced, 5);
        assert_eq!(decode(&priced).unwrap(), data, "priced variant not exact");
        assert_eq!(
            priced[4],
            Method::ResidualPriced as u8,
            "priced method was downgraded, so the comparison is meaningless"
        );
        let base = encode_tuned(&data, Method::Residual, 5);
        assert_ne!(
            priced, base,
            "the priced vocabulary produced the parent archive"
        );

        // And the pricing itself must be sane: a word the model predicts well is
        // priced lower than one it does not, at the same occurrence count.
        let stream = untokened_stream(&data, Method::Residual, 5);
        let (prices, avg_bpb, n) = price_stream(&stream, Method::Residual, 5);
        assert!(n > 0 && avg_bpb > 0.0);
        assert!(!prices.is_empty());
        for p in &prices {
            assert_eq!(
                p.gain_bits(),
                p.literal_bits - p.token_bits - p.def_bits,
                "gain is not literal - token - definition"
            );
            assert!(p.literal_bits >= 0.0);
            assert!(p.count >= 1);
        }
    }

    #[cfg(feature = "reorder")]
    #[test]
    fn non_page_input_downgrades_to_parent() {
        // Without ascending page ids the encoder must fall back to the parent
        // method, and the decoder must not sort.
        let data = b"no pages here at all\n".to_vec();
        let arch = encode(&data);
        assert_eq!(arch[4], Method::Sse3 as u8);
        assert_eq!(decode(&arch).unwrap(), data);
    }

    #[cfg(feature = "reorder")]
    #[test]
    fn reorder_methods_roundtrip_and_reorder() {
        let data = page_corpus();
        for m in [
            Method::ReorderId,
            Method::ReorderTitle,
            Method::ReorderSize,
            Method::ReorderStruct,
            Method::ReorderMinHash,
            Method::ReorderGreedy,
            Method::ReorderTemplate,
            Method::ReorderShuffle,
            Method::ReorderCategory,
            Method::ReorderTemplateKey,
            Method::ReorderCategorySet,
            Method::ReorderFull,
            Method::ReorderFullResidual,
            Method::Residual,
            Method::ResidualCtl,
        ] {
            let arch = encode_with(&data, m);
            assert_eq!(decode(&arch).unwrap(), data, "method {}", m.name());
        }
        // The title order must differ from the record order.
        assert_ne!(
            crate::reorder::encode(&data, crate::reorder::Order::Identity),
            crate::reorder::encode(&data, crate::reorder::Order::Title),
            "title order did nothing"
        );
    }

    #[test]
    fn struct_hoist_shrinks_markup() {
        let data = b"<page><title>X</title><id>1</id></page>\n".repeat(100);
        let raw = encode_with(&data, Method::RawCm);
        let hoisted = encode_with(&data, Method::StructHoist);
        assert!(hoisted.len() < raw.len());
    }

    #[test]
    fn sfx_append_and_extract() {
        let mut img = b"ELF fake binary".to_vec();
        let arch = b"pretend archive payload".to_vec();
        append_sfx(&mut img, &arch);
        assert_eq!(extract_sfx(&img).unwrap(), &arch[..]);
        assert!(extract_sfx(b"no payload here").is_none());
    }

    #[test]
    fn sfx_last_marker_wins() {
        let mut img = b"x".to_vec();
        img.extend_from_slice(SFX_MARKER);
        let arch = b"real".to_vec();
        append_sfx(&mut img, &arch);
        assert_eq!(extract_sfx(&img).unwrap(), &arch[..]);
    }
}
