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
            _ => return None,
        })
    }

    /// All methods, for exhaustive exactness testing.
    pub const ALL: [Method; 73] = [
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
        )
    }

    /// Methods measured on the Phase 6 parent: the accepted config plus the two
    /// adopted spine mechanisms (state-map experts and the extra SSE stage).
    fn base6(self) -> bool {
        matches!(
            self,
            Method::Phase6
                | Method::Spine
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
    fn token_kind(self) -> TokenKind {
        #[cfg(feature = "word-token")]
        {
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
fn maybe_token(method: Method, data: Vec<u8>) -> Vec<u8> {
    match method.token_kind() {
        TokenKind::None => data,
        TokenKind::Words => crate::transform::word_token_encode(&data, false),
        TokenKind::Reverse => crate::transform::word_token_encode(&data, true),
        TokenKind::V2 => crate::transform::word_token2_encode(&data),
        TokenKind::Phrase => crate::transform::word_token_phrase_encode(&data, true),
        TokenKind::PhraseFreq => crate::transform::word_token_phrase_encode(&data, false),
        TokenKind::Front => crate::transform::word_token_front_encode(&data, true),
        TokenKind::Affix => crate::transform::word_token_affix_encode(&data, true),
    }
}

#[cfg(not(any(feature = "word-token", feature = "word-token2")))]
fn maybe_token(_method: Method, data: Vec<u8>) -> Vec<u8> {
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
pub const ACCEPTED_METHOD: Method = Method::Sse3;
/// A20: mixer learning rate 24 was adopted on enwik7 screening and confirmed on
/// enwik8 (−76,483 B at zero executable cost).
pub const ACCEPTED_TUNE: u8 = 7;

/// Compress `input` into an archive payload using the accepted configuration.
pub fn encode(input: &[u8]) -> Vec<u8> {
    encode_tuned(input, ACCEPTED_METHOD, ACCEPTED_TUNE)
}

/// Compress with an explicit method (used by ablation and Phase-A runs).
pub fn encode_with(input: &[u8], method: Method) -> Vec<u8> {
    encode_tuned(input, method, 0)
}

/// Compress with an explicit method and optimizer tuning variant (A20).
/// `tune` selects a learning-rate/update variant at *runtime*, so it costs no
/// additional executable bytes; the value is stored in the header and the
/// decoder reconstructs the identical model.
pub fn encode_tuned(input: &[u8], method: Method, tune: u8) -> Vec<u8> {
    let lzbed = maybe_lzbe(method, input.to_vec());
    let grammared = maybe_grammar(method, lzbed);
    let stemmed = maybe_stem(method, grammared);
    let hoisted = maybe_hoist(method, &stemmed);
    let cased = maybe_case(method, hoisted);
    let tokened = maybe_token(method, cased);
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

    for &byte in data.iter() {
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
    let lzbed = maybe_lzbe(method, input.to_vec());
    let grammared = maybe_grammar(method, lzbed);
    let stemmed = maybe_stem(method, grammared);
    let hoisted = maybe_hoist(method, &stemmed);
    let cased = maybe_case(method, hoisted);
    let tokened = maybe_token(method, cased);
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
    let mut cm = Cm::new(&cfg, n);
    let mut enc = RangeEncoder::with_capacity(n / 2 + 64);
    for &byte in data.iter() {
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

/// Decode an archive payload produced by [`encode_with`].
pub fn decode(archive: &[u8]) -> Option<Vec<u8>> {
    if archive.len() < HEADER_LEN {
        return None;
    }
    if &archive[0..4] != MAGIC {
        return None;
    }
    let method = match archive[4] {
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
        _ => return None,
    };
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
    for _ in 0..n {
        let mut byte = 0u32;
        for _ in 0..8 {
            let p = cm.predict();
            let bit = dec.decode(p);
            cm.update(bit);
            byte = (byte << 1) | bit;
        }
        decoded.push(byte as u8);
    }

    let unpermuted = maybe_unperm(decoded, perm.as_ref());
    let untokened = maybe_untoken(method, unpermuted);
    let uncased = maybe_uncase(method, untokened);
    let unhoisted = maybe_unhoist(method, uncased);
    let unstemmed = maybe_unstem(method, unhoisted);
    let ungrammared = maybe_ungrammar(method, unstemmed);
    Some(maybe_unlzbe(method, ungrammared))
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
        let arch = encode(b"hello");
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
        assert_eq!(decode(&arch).unwrap(), b"hello");
    }

    #[test]
    fn tuning_variants_roundtrip() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(200);
        for tune in 0..8u8 {
            let arch = encode_tuned(&data, Method::StructHoist, tune);
            assert_eq!(decode(&arch).unwrap(), data, "tune={tune}");
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
