# Phase 14 — Procedural Entropy Compiler: the 80 MB campaign

> **Objective.** Reach `S ≤ 80,000,000` for `enwik9`, where `S` is the *complete
> charged submission*: program (charged twice by both legal forms) plus archive.
>
> **Baseline at `29ed675`** (reproduced, then used as authority):
>
> ```text
> archive9.bhm     160,015,425 B
> program (musl)       125,056 B
> S (separate)     160,265,537 B
> S (self-extract) 160,265,560 B
> tune 51 = scale 3 + mixer LR 8, adaptation ladder ACCEPTED_RATES,
> retrained 120 B residual corrector
> ```
>
> The target therefore requires **≈80 MB** of additional *complete* description-length
> reduction. Phase 13 (the re-test campaign) is paused by instruction; this phase
> attacks the representation itself.
>
> **Governing thesis.** Do not predict or store information that can be
> *reconstructed* from a shorter bounded generative explanation. Persist the
> explanation, persist its irreducible state, encode only the remaining
> innovation, and charge every byte required for exact reproduction:
>
> ```text
> X = Materialize(P, θ, R)
> L = L(P) + Σ L(θ_i | P) + Σ L(R_i | P, θ_i) + L(machinery)
> ```

## 0. Constitution (unchanged, restated because every decision depends on it)

1. **`S` is authority.** Not bpc, not ratio, not entropy, not coverage, not a
   projection from a smaller rung. `ΔS < 0` after exact reconstruction.
2. **Measured, never estimated.** Binary costs come from two-build comparisons
   (`A31`); a sub-kilobyte artifact delta is a *layout* property until proven
   otherwise.
3. **enwik9 decides.** enwik6/7/8 reject mechanics; they never license
   extrapolation upward. Scale reversals have occurred in both directions.
4. **Search has no decode authority.** Whatever the research plane discovers, the
   archive carries only the discovered explanation.
5. **Every bound is enforced, not asserted.** Depth, work, output, allocation,
   references. A forged archive must produce typed rejection, never panic/OOM/hang.
6. **Every command runs capped.** `tools/memcap.sh <1|4|8|10> …`. Concurrency is
   explicit and bounded; no unbounded multi-gate loops.
7. **A model that cannot pay for its own persisted bytes does not exist.**

## 1. Milestone gates (targets, not predictions)

```text
G0  reproduce the baseline exactly          <- this phase's first deliverable
G1  <= 150 MB
G2  <= 135 MB
G3  <= 120 MB
G4  <= 110 MB          (the accepted record is 110,793,128 B of S)
G5  <= 100 MB          (zmix publishes 99,312,424 B of S)
G6  <=  95 MB
G7  <=  90 MB
G8  <=  85 MB
T80 <=  80 MB          <- the objective
```

Each gate is claimed only by a produced, exact, resource-compliant artifact, with
`Δarchive`, `Δbinary`, model/program/state/residual bytes, RAM, wall time and
ablation evidence recorded.

## 2. Kill gates (anti-sunk-cost)

| gate | condition | action |
|---|---|---|
| A | `program + state + residual ≥ accepted` on the class | reject the representation |
| B | grammar coverage large, archive gain negligible | reject or redesign |
| C | fully costed realizable floor stays `> 95 MB` after structural + lexical work | introduce a new representation family, do not tune toward 80 |
| D | program/state tiny, residual enormous | shift to residual/language modelling |
| E | `data saving ≤ model bytes + binary cost` | reject the model |
| F | search cannot complete inside judged constraints | reduce or replace search |

## 3. Subphases

Each subphase ends in **ADOPT / REJECT / REDESIGN** before the next begins where
dependencies allow, and each is committed and pushed to `staging/phase14`.

| # | subphase | deliverable | gate |
|---|---|---|---|
| **P14.0** | Baseline + scaffolding | reproduce the authority numbers; this plan; gate policy | G0 |
| **P14.1** | Byte/codelength attribution | `zentropy opportunity <corpus>`: actual *coded* bytes attributed to every structural/lexical class and to each predictor role; every byte attributable | measurement |
| **P14.2** | Realizable lower-bound oracle | `zentropy opportunity <corpus> --oracle`: per class, current cost, zeroth-order bound, causal conditional bound, best candidate cost — all *fully charged*; `docs/PHASE14_FIRST_BOUNDARY.md` skeleton | interpretation table §14.5 |
| **P14.3** | `SignalBus` | one causal structural-state interface, decoder-derivable, shared by every consumer | causality + determinism tests |
| **P14.4** | Procedural VM | `src/procedural/`: `Literal`, `Concat`, `Repeat`, `Ref`, `Slice`, `Template`, `Patch` with exact serialization and enforced bounds | decoder-safety court |
| **P14.5** | Program-cost authority | exact serialized program bytes as the search objective | cost equality test |
| **P14.6** | Target-directed search | backward decomposers + bounded A*/branch-and-bound with `Literal` as incumbent; final selection on *actual serialized bytes* | search-soundness tests |
| **P14.7** | State coding competition | raw / varint / delta / rank / block-rank / rANS; rank-unrank primitives (mixed-radix, subset, permutation, monotone) | lowest complete cost wins |
| **P14.8** | Typed residual algebra | `None`/`SparseSubstitute`/`RangeReplace`/`RunPatch`/`RankedMismatch`/`EditScript`, with WHERE and WHAT coded separately | residual court |
| **P14.9** | **First boundary experiment** | a real Wikipedia class where `program + state + residual + marginal decoder` beats the accepted representation, with a negative control; `docs/PHASE14_FIRST_BOUNDARY.md` complete with CONTINUE / REDESIGN / STOP | **the phase's pivotal decision** |

Contingent subphases (only if P14.9 says CONTINUE), in the §14.50 order:
P14.10 structured universes, P14.11 corpus-native reference tables, P14.12 shared
program DAG, P14.13 recursive residual proceduralization, P14.14 procedural-aware
article order, P14.15 morphology/phrase families, P14.16 `ContextMap` +
`MatchTrust`, P14.17 deep PPM distribution expert, P14.18 hierarchical mixer,
P14.19 temporal learned residual expert, P14.20 model-size-aware quantization,
P14.21 joint interaction/ablation campaign, P14.22 global re-optimization,
P14.23 submission-resource closure, P14.24 the 80 MB authority campaign.

## 4. Frozen interfaces (the contract parallel work is written against)

These signatures are fixed for P14.1–P14.9. Changing one is a deliberate,
committed interface change with its call sites updated in the same commit.

### 4.1 `SignalBus` — `src/signal.rs`

```rust
/// Causal, decoder-derivable structural state at the current byte boundary.
/// Every field is a pure function of bytes already coded, or of state that was
/// explicitly persisted in the archive. Never a function of the future.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SignalState {
    pub byte_pos: u64,
    pub bitpos: u8,          // 0..8 within the current byte
    pub zir: ZirClass,       // span type from the corpus IR
    pub xml: XmlState,       // inside tag / attr / text / cdata
    pub page: PageState,     // inside page / revision / text / section
    pub depth_page: u8,      // bounded nesting counters
    pub heading: u8,         // 0 = none, 1..=6
    pub template_depth: u8,
    pub template_id: u32,    // local id, 0 = none
    pub link: LinkState,
    pub table: TableState,   // row/column state, 0 = outside a table
    pub list_depth: u8,
    pub word: WordState,     // inside a word + word-local position
    pub numeric: bool,
    pub residual_class: u8,
    pub program_id: u32,     // procedural context, 0 = none
}

/// The interface. One instance per coding pass; consumers read, the driver
/// updates it as bytes are consumed.
pub trait SignalBus {
    fn observe(&mut self, byte: u8);
    fn state(&self) -> SignalState;
    /// Context value for a model that wants a cheap integer key from a field
    /// subset. Stable for a given state; not a hash of memory.
    fn key(&self, fields: SignalFields) -> u32;
}
```

### 4.2 Procedural VM — `src/procedural/`

```rust
pub enum Op {
    Literal(LiteralId),
    Concat { parts: SmallVec },          // bounded arity
    Repeat { child: NodeId, count: u32 },
    Ref { target: RefTarget },           // decoded material, by local id
    Slice { src: NodeId, from: u32, len: u32 },
    Template { slots: SmallVec, program: NodeId },
    Patch { base: NodeId, residual: ResidualId },
}

pub struct Program { pub nodes: Vec<Node>, pub root: NodeId }
pub struct Node { pub op: Op, pub span: Span }   // Span = target extent covered

/// Exact serializer. The ONLY cost authority for a program.
pub fn serialize(p: &Program) -> Vec<u8>;
pub fn deserialize(b: &[u8]) -> Option<Program>;

/// Bounds enforced at deserialize AND at execute; a violation is a typed error.
pub struct Bounds { pub max_depth: u8, pub max_nodes: u32,
                    pub max_output: u64, pub max_work: u64, pub max_refs: u32 }

pub enum ExecError { Depth, Nodes, Output, Work, RefCycle, BadRef, BadRank }

/// Materialize exactly, or fail typed. Never partially.
pub fn execute(p: &Program, cx: &ExecContext, bounds: &Bounds)
    -> Result<Vec<u8>, ExecError>;
```

### 4.3 State coding

```rust
pub enum StateCodec { Raw, Varint, Delta, Rank, BlockRank, Rans }

/// Cost in *serialized bytes*, not bits-estimate.
pub fn encode_state(codec: StateCodec, theta: &State, p: &Program) -> Vec<u8>;

// rank/unrank primitives
pub fn rank_subset(n: u32, k: u32, set: &[u32]) -> u64;
pub fn unrank_subset(n: u32, k: u32, r: u64) -> Option<Vec<u32>>;
pub fn rank_permutation(perm: &[u32]) -> u64;
pub fn unrank_permutation(n: u32, r: u64) -> Option<Vec<u32>>;
pub fn rank_mixed_radix(radices: &[u32], digits: &[u32]) -> Option<Vec<u64>>;
```

### 4.4 Residual algebra

```rust
pub enum Residual {
    None,
    SparseSubstitute { positions: Vec<u32>, values: Vec<u8> },
    RangeReplace { ranges: Vec<(u32, u32)>, data: Vec<u8> },
    RunPatch { runs: Vec<(u32, u32, u8)>, },
    RankedMismatch { mask_rank: u64, values: Vec<u8> },
    EditScript { ops: Vec<EditOp> },
}
pub fn apply(base: &[u8], r: &Residual) -> Option<Vec<u8>>;
pub fn derive(base: &[u8], target: &[u8], kind: ResidualKind) -> Option<Residual>;
```

### 4.5 Opportunity attribution

```rust
/// Accumulates real coded bits per class. Research plane only; compiled out of
/// the scored build.
#[cfg(feature = "opportunity")]
pub struct Opportunity { /* per-class cost accumulators */ }
```

Classes (exactly the §14.4 tree): `Executable, TransformState, RestorationState,
XmlStructure, TitlesIds, RevisionMeta, Templates, TemplateParams, Links,
Categories, References, Tables, Lists, Numbers, Lexical, Punctuation, MatchResidual,
DictState, LearnedModel, Unclassified` — plus per-predictor-role attribution
(`Literal, Match, Word, WordBigram, OrderModel, Sse, LearnedCorrection`).

Every byte must land in exactly one structural class and one role; the report
asserts `Σ classes == total` and `Σ roles == total`, so an unclassified byte is a
bug, not a rounding error.

## 5. Controls (mandatory, per claim)

`Literal`-only search · random cohorts with identical size distribution · best
actual member as prototype base (vs synthesized) · independent state coding at
equal semantics · structural context keyed on an unrelated state at equal table
size · same-size constant match-trust history · zero/permuted/random-weight
learned models · uniform search prior at equal budget · identity/shuffle/current
article order.

No claim is accepted without its falsifier, and large claims must decompose:
"proceduralization −7.1 MB" is not an acceptable attribution; program sharing,
ranked state, prototypes, typed residual, shared rANS models and cohorting are
each reported separately.

## 6. Repository discipline for this phase

- One Rust package, no new crate dependencies in the scored path.
- Research-plane code is feature-gated and outside `accepted`.
- `src/procedural/**` and `src/signal.rs` are separate modules registered in
  `lib.rs`; the scored build includes only what pays for itself.
- Every subphase: exactness court → ladder → **enwik9 authority gate** → receipt →
  commit → push.
- `docs/PHASE14_FIRST_BOUNDARY.md` is written at P14.9 and ends with exactly one
  of `CONTINUE`, `REDESIGN`, `STOP PROCEDURAL FAMILY`, with quantitative reasons.
