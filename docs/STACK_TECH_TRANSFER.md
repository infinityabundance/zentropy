# Zentropy — Stack Technology Transfer (conceptual transfer with receipts)

> Purpose. Zentropy is **not** a fork, wrapper, or concatenation of the author's prior
> systems. This document extracts the strongest *underlying mechanisms* from the
> inspected prior art, rederives them for text compression, and admits a mechanism
> only if it lowers the complete Hutter score
> `S = submitted_compressor_bytes + self_extracting_archive_bytes` after exact
> reconstruction. Every claim below is grounded in a file that was actually read;
> ungrounded items are marked `UNRESOLVED_REFERENCE` rather than asserted.

## 0.1 Method and evidence base

Each archive was extracted to a scratch directory **outside** the deliverable and read
in source form. PDFs were text-extracted with `pdftotext` where no `.tex`/`.md` source
was present; where a `.tex` source existed it was preferred. This bulk text was read
from the extracted trees, and each row's citation names the file **inside the archive**
(as `research/<project>/<archive>.zip:<path>`) or the loose file in `research/`.

### Inspection ledger (verified vs missing)

| Project | Snapshot inspected | Mechanism verified from source? | Notes |
|---|---|---|---|
| EntropyFS | `research/entropyfs/entropyfs-main(6).zip`; `research/entropyfs/entropyfs_entropy_native_configurational_storage.tex`; `Entropyfs-Video-Descriptor.md` | **yes** | Phase 12D-0/12D-1 oracle, rank/unrank families, cost model, accounting, DSFB observer, rANS backend ADR all read in source/docs. |
| VOLE | `research/vole/vole-main.zip`; `research/vole/vole_procedural_video_prior_art.tex` | **yes** | Phase-G inverse proceduralization, Phase-N generators, Phase-O equivalence-preserving rewrites, empirical-status ledger with measured numbers. |
| VOLE-Camera | `research/vole-camera/vole-camera-main.zip`; `research/vole-camera/vole_camera_prior_art.tex` | **yes** (Phases A–B only in snapshot) | Architecture, non-claims, flattening-tax formula, RAW10 boundary adapters read; later phases absent from snapshot, recorded as such. |
| VOLE-GFX | `research/vole-gfx/vole-gfx-main(1).zip`; `research/vole-gfx/vole_gfx_prior_art.tex` | **yes** (Phases A–K implemented) | Inverse compiler, exact semantics, Pareto frontier source, search-work accounting, SIMD-parity doctrine read. Phases L–N pending. |
| VOLE-Audio | `research/vole-audio/vole-audio-main(2).zip`; `research/vole-audio/vole_audio_prior_art.tex`; `phase O.txt` | **yes** (Phase H.2/K/L implemented; O is a plan) | Entropy-native layer, inverse compiler, complete-cost accounting, RAW-fallback gate read; the learned-prediction addendum is a written plan, not implemented code. |
| VOLE-Field | `research/vole-field/vole-field-prior-art.md`; `vole_field_prior_art.tex` | **yes** | Core formalism, representation authority, inverse compilation, flattening tax, EntropyFS/DSFB layers, non-claims read. |
| FRF | `research/Forensic Residual Framework/frf-main(5).zip`; `forensic_residual_framework.tex`; `frf skill/frf-v2-SKILL.md`; `frf-quick-reference.md` | **yes** | Kernel, courts, mutation/challenge, minimizers, κ endoduction, trajectory classification, receipts, deterministic fuzz harness read. |
| frf-fuzz | **absent as a separate snapshot** | **partial** | Not present as a repo. FRF itself ships a deterministic seeded fuzz harness and libFuzzer targets; a distinct “frf-fuzz” product is referenced by VOLE-Field but its source was not available. See §6 and §10. |
| DSFB | `research/dsfb/dsfb-main.zip` | **yes** (core `dsfb` crate + framework docs) | `observer.rs`/`trust.rs`/`state.rs`/`params.rs` read; monorepo has dozens of application crates not all inspected. |
| Gemel | **absent** | **no** | No snapshot. Intended role recoverable only from VOLE-Field’s citations. See §8 and §10. |
| ryg-rans-rs | `research/ryg-rans-rs/ryg-rans-rs-main.zip` | **yes** | Core surfaces, bitstream contract, residual doctrine, ADRs, Kani proofs inventory read. |
| EntropyFS Video Descriptor | `research/entropyfs/Entropyfs-Video-Descriptor.md` | **yes** (draft RFC) | `MOTION_FIELD_RANK`, `SPARSE_RESIDUAL`, `FRAME_BLEND_XOR`, O(1) seekable frame descriptors read. |

### Status legend (Zentropy transfer status, not source-project status)

- `PROPOSED` — a concrete transfer hypothesis awaiting a Zentropy court.
- `MEASURED` — a Zentropy-side measurement exists (initially none; source-side
  measurements are cited in prose and are **not** conflated with this column).
- `ADOPTED` — a zero-scored-byte constitutional requirement that is already binding on
  the architecture (accounting law, exactness authority, literal fallback). These are
  admitted because they cost no scored bytes and prevent invalid submissions.
- `REJECTED` — an anti-mechanism the transfer must not accept.
- `SUPERSEDED` — a later mechanism in the inspected stack subsumes it.

`Estimated byte cost` is scored bytes added to `S`; `estimated runtime cost` is judged-path
runtime; `expected information gain` is an **engineering estimate (EST)**, to be
replaced by a measured ΔS. No estimate here is a claim.

---

# Part I — Per-project transfer tables

## 1. EntropyFS — entropy-native configurational storage

EntropyFS is a filesystem whose durable object is a *bounded deterministic representation
descriptor*, not the logical bytes: `X = Materialize(D)`
(`research/entropyfs/entropyfs-main(6).zip:docs/theory/entropy-medium.md`). The descriptor
language is a closed, non-Turing-complete set of representations — `ZERO`, `FILL`, `RAW`,
`RANS`, `EXACT_REF`, `BASE_RESIDUAL`, `SPARSE`, `PALETTE`, `PERIODIC`, `ENTROPY_REF`,
`INLINE`, `PERMUTATION`, `SEQUENCE_RANS`, `SPARSE_BLOCK64`, plus dictionary/deep sequence
extensions (`...:docs/adr/0005-representation-set.md`). Candidate selection minimizes an
explicit persisted-byte cost (`...:docs/adr/0010-cost-function.md`), and every persistent
bit is bucketed as descriptor/model/residual/seed/reference/configurational/integrity
(`...:docs/theory/information-accounting.md`). The object model is an immutable
content-addressed graph with copy-on-write mutation (`...:docs/adr/0007-object-model.md`).
The research value for Zentropy is not the filesystem but three primitives: exact
**configuration rank/unrank**, exact **grammar + state + residual**, and a **complete-cost**
selector that refuses to hide side state. Its `ENTROPY_REF` universe is deliberately paired
with a random-XOF **negative control** so that “a seed materializes a gigabyte” cannot be
claimed (`...:docs/theory/entropy-medium.md`). The draft Video Volume Descriptor shows the
same object model specialized to video, with `MOTION_FIELD_RANK` storing a rank in a
smooth-field state space and independent per-frame descriptors for O(1) seek
(`research/entropyfs/Entropyfs-Video-Descriptor.md`).

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| EntropyFS | Descriptor language `X = Materialize(D)` (`0005-representation-set.md`) | Logical bytes are a materialized view of a bounded descriptor | no (rederive) | Template for ZIR op set | ZIR opcode grammar | 0 scored (format only) | none | Defines the representation search space | Adopt bounded op set + universal `RAW` | ADOPTED |
| EntropyFS | Combinatorial rank/unrank `SPARSE`/`PALETTE`/`PERMUTATION`/`PERIODIC` (`docs/theory/configurational-storage.md`, `src/entropy/{sparse,palette,permutation,periodic,rank}.rs`) | Persist the *coordinate* in a constrained state space instead of bytes | **yes** (port algorithms, re-derive for text) | Enumerative coder candidate; exact `rank`/`unrank` | `RankedState`/`EnumerativeState` opcodes; checked overflow → reject | ~1–3 KB code per family; payload = `ceil(log2 N_states)` | cheap integer arithmetic | High on sparse/low-cardinality structured spans | Ablate rank vs rANS on sparse markup/attribute positions | PROPOSED |
| EntropyFS | `SEQUENCE_RANS` / `SEQUENCE_DICT` / `SEQUENCE_SHARED_DICT` / `SEQUENCE_DEEP` (LZ + rANS streams) (`0005-representation-set.md`) | Match/literal/offset streams, each entropy-coded or raw, with repeat-offset recency | **yes** (pattern) | Match-model + repeat-offset candidates | `LongMatch`/`RepeatedOffsetMatch`/`SparseMatch` + `Sequence` | ~5–15 KB code; streams in archive | O(n·depth) search (encoder-only) | High; measured 3.786× standalone floor | Adopt recency/offset state; measure enwik residual | PROPOSED |
| EntropyFS | Grammar-addressed entropy: `G` skeleton stored once, per-member slot state + descriptor (`docs/performance/grammar-oracle.md`, `src/tests/grammar_oracle.rs`) | Shared template grammar + per-instance production state + exact residual | **yes** (concept; rederive inducer for wiki constructs) | Grammar induction over article families | `GrammarProduction`/`Template` opcodes | grammar + state + descriptor, fully charged | induction offline; decode bounded | **12D-1 measured**: skeleton cost −47%; still 1.18× behind zstd-whole | Port to wiki infobox/citation families; re-close the 2 named gaps | PROPOSED |
| EntropyFS | Raw-literal grammar skeleton (12D-0) (`grammar-oracle.md`) | Template grammar stored with a literal skeleton | no (superseded) | Retained as the conservative 12D-0 bound | none | 66,059 B on the 12D corpus | cheap | — | Already measured; superseded by 12D-1 | SUPERSEDED |
| EntropyFS | Entropy-code the grammar object itself (`grammar-oracle.md` 12D-1) | The grammar is data and must be entropy-coded | **yes** (rule) | Same proof as above | Mandatory: never store a skeleton as literal | saves the grammar’s own redundancy | entropy decode | −47% on the 12D corpus | Confirm on text grammars | ADOPTED (as a law) |
| EntropyFS | Rank-code per-instance state (`grammar-oracle.md` 12D-1 remaining gap) | Slot values are constrained state, not free bytes | **yes** | Rank candidates for slot vectors | `RankedState` state encoding | ~2–3 KB saved in 12D corpus | cheap | Medium, explicitly identified | Rank-code template slot vectors | PROPOSED |
| EntropyFS | Second-order+ contextual coder for the skeleton (`grammar-oracle.md` 12D-1 gap 1) | Order-1 sequence modeling leaves ~0.18 bits/byte on the table | **yes** (design) | Context-model experiments | Context-mixing floor | code cost of CMs | judged-path CPU | Medium–high; the *only* path to the format bit in 12D | Build order-2+ contextual coder, ablate | PROPOSED |
| EntropyFS | Complete per-extent accounting buckets (`information-accounting.md`) | No mechanism is credited with another’s savings; no hidden side state | **yes** (discipline) | ΔS attribution harness | Score calculator + ablation ledger | 0 | none | Prevents false wins | Freeze bucket schema | ADOPTED |
| EntropyFS | Explicit cost function `J` (`adr/0010-cost-function.md`) | Select by full persisted cost + policy weights, not bytes alone | partial | Candidate ranking | Complete-cost selector (`λ` for Hutter) | 0 | selection | Prevents regression-by-side-state | Map λ to Hutter (bytes-dominant) | ADOPTED |
| EntropyFS | `ENTROPY_REF` + `UniformXofV1` negative control (`entropy-medium.md`) | A generator is only useful if the residual closes cheaply | **yes** (method) | Negative control for any generator claim | Reject any “seed magic” mechanism | 0 | XOF search forbidden | Prevents pathological overfit | Include random-XOF negative control | ADOPTED |
| EntropyFS | DSFB-guided candidate ordering (`docs/theory/dsfb-selection.md`, `adr/0004`) | Observers may order search, never decide decode | **yes** (discipline) | Candidate budget ordering | none (never in decoder) | 0 scored | may reduce search CPU | Search-cost only | Measure vs fixed heuristic on text | PROPOSED |
| EntropyFS | Video `MOTION_FIELD_RANK` / independent frame descriptors (`Entropyfs-Video-Descriptor.md`) | A structured field may be stored as a rank in its plausible-field space; descriptors seek independently | partial (concept) | Rank families for typed fields | `RankedState` for numeric/structural fields; independent article/segment descriptors | rank bits = `log2 C(n,k)` | unrank cost | Medium–high on structured numeric/markup fields | Rank-code field configurations | PROPOSED |
| EntropyFS | Immutable content-addressed object graph + COW (`adr/0007`) | Reuse requires identical bytes; identity is a hash, not a heuristic | partial | Dedup/experiment store | `REUSE_BY_ID` model/state references | 32 B/reference | O(1) lookup | Medium on repeated models/grammars | Model/table reuse accounting | PROPOSED |

## 2. VOLE — inverse proceduralization

VOLE stores a bounded deterministic procedural state graph `G_t`, evolves it by explicit
transitions `Φ`, and materializes views, with an explicit residual algebra closing the gap:
`F_t = M(U,G_t,V) ⊕_ρ R_t` (`research/vole/vole-main.zip:docs/architecture.md`,
`docs/residuals.md`). The decisive mechanism for Zentropy is **inverse proceduralization**:
the Phase-G encoder consumes an observed raster sequence and, per frame, *exhaustively*
evaluates bounded declarative candidate programs, materializes each through the normative
materializer, **keeps only byte-exact explanations**, and emits the complete-cost winner
(`...:docs/phase-g.md`; `examples/inverse_proof.rs`). Candidate generation, evaluation, and
admission are strictly separated: a heuristic may propose, only materialization + exact
comparison admits. Phase-O adds **equivalence-preserving re-optimization**: velocity
collapse, trajectory collapse, residual promotion, generator substitution, and duplicate
merge, each accepted only when the rebuilt stream is *strictly smaller* and decodes
byte-identically (`...:docs/phase-o.md`). Phase-N **bounded procedural generators** are only
accepted after the normative render is compared byte-for-byte; an inexact fit is admissible
only as `generator_residual` with its correction counted (`...:docs/phase-n.md`). The
empirical-status ledger publishes measured flattening taxes and DSFB search results
(`...:docs/empirical-status.md`).

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| VOLE | Inverse proceduralization loop (`docs/phase-g.md`; `examples/inverse_proof.rs`) | observation → candidates → materialize → exact residual closure → complete-cost winner | **yes** (rederive in Rust for text) | Inverse compiler over text spans | Candidate admission + ZIR emission | 0 extra (search is encoder-side) | high encode CPU, bounded decode | Foundational: converts model choice into exact competition | Build text inverse compiler over spans | PROPOSED |
| VOLE | Exhaustive bounded candidate families (`docs/phase-g.md`) | Every candidate is a declarative program; winner = min persisted bytes | **yes** | Candidate enumerator | ZIR ops | 0 | encoder-only | High | Reimplement families for text (§Part II.2) | PROPOSED |
| VOLE | Whole-frame RAW recapture / frame-0 raster tax (Phase G) | Rebasing an entire frame at appearance | no (superseded) | Retained as the recorded Phase-G baseline | none | frame-0 full-raster cost | decode | — | Superseded by Phase-K variable regions (zero whole-frame rebases after frame 0) | SUPERSEDED |
| VOLE | Exact residual algebra with explicit `ρ` (`docs/residuals.md`) | The residual operation is declared, never implied | **yes** | Residual kind experiments | `ResidualStream` algebra tag | 1 tag byte | cheap | Prevents ambiguity | Fix residual algebra(s) for text | PROPOSED |
| VOLE | Equivalence-preserving rewrite to fixpoint (`docs/phase-o.md`) | Rewrites accepted only if strictly smaller **and** decode-identical | **yes** (design) | Representation optimizer | Phase-10 optimizer | reduces `S` | encoder-only | Medium–high | Port rewrite family list to ZIR | PROPOSED |
| VOLE | Generator substitution with exact render check (`docs/phase-o.md`, `phase-n.md`) | A stored object is replaced by its program only if the program reproduces it exactly | **yes** | Generator discovery | `GeneratorState` opcode | program vs bytes | decode compute | High on repetitive/generative spans | Fit generators to wiki templates | PROPOSED |
| VOLE | Inexact-fit gate ≥ 15/16 pixels, else `generator_residual` with counted correction (`docs/phase-n.md`) | An approximation is only allowed with its exact correction charged | **yes** | Fit acceptance | Generator + residual pair | counted residual | decode compute | Medium | Re-derive threshold for text constructs | PROPOSED |
| VOLE | Measured flattening tax (`docs/empirical-status.md` Phase Q rows) | Destroying source structure then reinferring it costs more | **yes** (as evidence, not code) | Motivates source-native IR | None (informs IR design) | 0 | 0 | Justifies Phase-1 IR priority | Re-measure on Wikipedia | PROPOSED |
| VOLE | DSFB search governance with regret receipts (`docs/dsfb-search.md`; `docs/empirical-status.md`) | `N_dsfb < N_exhaustive` while `J_dsfb ≈ J_exhaustive` | **yes** (evaluation protocol) | Search budget policy | none | 0 scored | may cut encoder CPU | Search-cost only | Ablate on Zentropy candidate set | PROPOSED |
| VOLE | 10-bucket complete accounting (`docs/information-accounting.md`) | All persisted bytes compared, shared objects never zeroed | **yes** | Accounting harness | Score accounting | 0 | 0 | Prevents hidden state | Adopt bucket schema | ADOPTED |
| VOLE | Content-addressed store + declared/unique/physical (`docs/empirical-status.md` Phase P) | Reuse is a measured delta, not an assumption | partial | Experiment artifact store | `REUSE_BY_ID` models | reference bytes | lookup | Medium | Adopt model/content addressing | PROPOSED |

## 3. VOLE-Camera — sensor-native ingest and the flattening tax

VOLE-Camera is a separate crate that owns only the camera-specific layer and asks whether
starting the representation at the sensor preserves deterministic state that is lost when
observations are first flattened into conventional frames
(`research/vole-camera/vole-camera-main.zip:docs/architecture.md`). It distinguishes
**source-native** from **flattened-inverse** paths and reports
`T_flatten = B_inverse / B_native` with all four byte counts, never only the ratio
(`...:docs/non-claims.md`). The residual is “central output, not embarrassment”; a
residual-dominant result is a recorded success. The canonical sensor sample is a 10-bit
value in `u16`, and RAW10 packing is explicitly a *boundary* transport layout, not canonical
identity (`...:src/raw.rs`). For Zentropy the transferable lesson is negative-space: a
byte-oriented enwik9 pipeline that flattens XML/wiki structure into an undifferentiated
stream and only later tries to rediscover structure pays the flattening tax. The snapshot
implements Phases A–B only; later factorizer/materializer phases are absent and must not be
assumed.

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| VOLE-Camera | Source-native vs flattened-inverse as separately measured paths (`docs/non-claims.md`) | Do not destroy known structure then re-infer it | **yes** (method) | Justifies source-native Wikipedia IR | Phase-1 IR | 0 | 0 | Foundational | Build + measure both paths on enwik slices | PROPOSED |
| VOLE-Camera | Flattening tax `T_flatten = B_inverse / B_native` (`docs/non-claims.md`) | The cost of premature flattening is measurable | **yes** (metric) | Experiment metric | None (research metric) | 0 | 0 | Quantifies IR value | Report `T_flatten` for enwik IR vs raw | PROPOSED |
| VOLE-Camera | Boundary representation ≠ canonical identity (`src/raw.rs`) | Transport encodings must not leak into semantic identity | **yes** (discipline) | IR design rule | Canonical IR must be byte-restorable | 0 | 0 | Prevents lossy normalization | Enforce canonical-form checks | ADOPTED |
| VOLE-Camera | Residual is not failure; residual-dominant outcomes are recorded (`docs/non-claims.md`) | Negative/weak-explanation results are first-class evidence | **yes** (discipline) | Evidence ledger | None | 0 | 0 | Prevents overfitting narratives | Record literal-fallback rates | ADOPTED |
| VOLE-Camera | Epistemic categories `PROVEN/MEASURED/PROPOSED/SPECULATIVE` (`docs/non-claims.md`) | Claims cannot be silently upgraded | **yes** (discipline) | Research-plane claim tagging | None | 0 | 0 | Improves research honesty | Adopt in `docs/` | ADOPTED |
| VOLE-Camera | Camera telemetry as candidate causal state, never proven causality (`docs/non-claims.md`) | Explanations are bounded computational explanations, not truth | **yes** (discipline) | Models prior art as hypotheses | None | 0 | 0 | Prevents semantic overreach | n/a | ADOPTED |

## 4. VOLE-GFX — search the explanation space

VOLE-GFX defines one mathematical semantics with many execution widths, where the scalar
implementation is the semantic oracle and accelerated backends must be **byte-for-byte**
identical (`research/vole-gfx/vole-gfx-main(1).zip:docs/ARCHITECTURE.md`). Its inverse
compiler maps an asset `A → (Γ, s, θ, R)`: detectors *propose*, evaluation materializes and
measures the exact residual, and a **Pareto frontier** over deterministic axes
(`persistent_bytes`, `residual_bytes`, `materialize_work`, `search_work`) admits candidates
(`...:docs/INVERSE_COMPILER.md`; `src/inverse/candidate.rs`, `frontier.rs`). Seeded-field
search accepts a seed only when the field equals the surface on **every** sample; SIMD is a
pruning/wall-clock accelerator whose accepted set must equal the scalar oracle’s
(`...:src/inverse/search.rs`). Non-claims state plainly that the compiler does not recover an
author’s “true semantics” and that “no generator explains pixels unless exact reconstruction
follows from counted deterministic state”
(`...:docs/NON_CLAIMS.md`). This is the exact authority split Zentropy needs between a large
GPU/search plane and a small deterministic reconstructor.

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| VOLE-GFX | Scalar oracle is the spec; accelerated backends must match byte-for-byte (`docs/ARCHITECTURE.md`) | Search acceleration never defines correctness | **yes** (doctrine) | GPU/SIMD proposal engines | Only the scalar exact reconstructor | 0 | CPU-only judged path | Prevents backend drift | Differential scalar/SIMD/GPU tests | ADOPTED |
| VOLE-GFX | Deterministic Pareto frontier, no weighted score (`src/inverse/frontier.rs`) | Multi-objective costs are not collapsed into a magic scalar | partial | Multi-axis experiment scoring | Complete-cost profile selection | 0 | 0 | Prevents opaque trade-offs | Emit Pareto rows per candidate | PROPOSED |
| VOLE-GFX | `search_work = discovery + discrimination` (`docs/INVERSE_COMPILER.md`) | Real search cost is counted, not estimated | partial | Experiment cost accounting | None (encoder-side) | 0 | encoder CPU accounting | Enables honest search-vs-gain analysis | Adopt search counters | PROPOSED |
| VOLE-GFX | Host-independent acceptance predicate (`src/inverse/search.rs`) | SIMD prunes but cannot accept/reject | **yes** | Batch candidate screening | Decode never depends on pruning | 0 | less encode time | Prevents false accepts | Differential accepted-set tests | ADOPTED |
| VOLE-GFX | Detectors are proposals; only materialize+compare admits (`docs/INVERSE_COMPILER.md`) | candidate generation ≠ semantic authority | **yes** | Grammar/dictionary/match proposers | Exact reconstructor admits | 0 | encoder-only | Core separation of concerns | Enforce in ZIR admission API | ADOPTED |
| VOLE-GFX | Pathological-overfit guard in non-claims (`docs/NON_CLAIMS.md`) | A generator “explains” only if counted state reconstructs exactly | **yes** (guard) | Overfit detection | Rejection rule | 0 | 0 | Prevents reviewer-visible overfit | Negative-control corpora | ADOPTED |

## 5. VOLE-Audio — explanation + innovation

VOLE-Audio’s corrected architecture is “store the deterministic explanation; entropy-code
what the explanation cannot reproduce; materialize sample-domain observations only when
required” (`research/vole-audio/vole-audio-main(2).zip:docs/ENTROPY_NATIVE.md`). Entropy
coding is **orthogonal to the hypothesis family** — a representation may be literal,
procedural, predictor+residual, or (Phase O) learned hypothesis + entropy-coded residual.
The inverse compiler accepts a candidate only when **two independent reconstructions** agree:
intrinsic closure and scalar-oracle observation; “close” is never accepted
(`...:docs/INVERSE.md`, `src/inverse/observe.rs`). Every candidate reports a **complete**
cost `metadata + hypothesis + model + payload + index + checkpoints + dependency + integrity`
and chooses rANS only when `complete_rans_bytes < complete_raw_bytes`
(`...:docs/ENTROPY_ACCOUNTING.md`). Failed procedural hypotheses (harmonic, FM, correlated,
transient, and all negative controls) are reported as literal wins — negative results are
results (`...:docs/INVERSE.md`). The written Phase-O addendum makes a **learned
deterministic prediction model just another candidate family**, judged by the same cost API,
with residual-aware training, deterministic integer inference, and scalar authority retained
(`research/vole-audio/phase O.txt`). The O artifact is a plan, not implemented code.

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| VOLE-Audio | `observation = deterministic explanation + entropy-coded innovation` (`docs/ENTROPY_NATIVE.md`) | Explain first, entropy-code the remainder | **yes** (architecture) | Flagship cascade design | Stage 1–3 then Stage 6 coder | model+residual only | staged | Foundational | Build the residual-prediction cascade | ADOPTED (as a design law) |
| VOLE-Audio | Entropy coding orthogonal to hypothesis family (`docs/ENTROPY_NATIVE.md`) | rANS is not itself a semantics | **yes** | Backend independence | Per-stream coder choice | table bytes | coder cost | High | Per-stream coder selection by complete cost | PROPOSED |
| VOLE-Audio | Two independent exact reconstructions before acceptance (`src/inverse/observe.rs`) | Intrinsic closure **and** oracle observation must both equal the target | **yes** | Candidate validator | Decode-court analogue | 0 | validation | Prevents silent divergence | Dual-path acceptance tests | ADOPTED |
| VOLE-Audio | `L_complete` eight-part cost (`docs/ENTROPY_ACCOUNTING.md`) | Never report payload alone | **yes** | Cost API | `S` components | 0 | 0 | Prevents model-byte denial | Adopt complete-cost API | ADOPTED |
| VOLE-Audio | `RANS` only if `complete_rans < complete_raw` (`docs/ENTROPY_ACCOUNTING.md`) | Literal fallback is a success condition | **yes** | Coder experiments | Per-stream coder choice | chooser overhead | coder selection | Medium | Coder-selection tournament | PROPOSED |
| VOLE-Audio | Deterministic Pareto `(complete_bytes, total_ops, seek_ops)` (`src/inverse/frontier.rs`) | Static, reproducible objectives only | partial | Candidate scoring | Complete-cost + resource gate | 0 | 0 | Prevents wall-clock lottery | Emit static objective rows | PROPOSED |
| VOLE-Audio | Explanation vs deduplication separation (`docs/INVERSE.md` §6) | A reference does not explain; it points at stored content | **yes** | Ablation correctness | `ExactReference` vs hypothesis distinction | 32 B reference | lookup | Prevents inflated “explanation” claims | Separate courts | ADOPTED |
| VOLE-Audio | Phase-O learned hypothesis family, residual-aware training (`phase O.txt`) | Learned state competes as one more exact-validated family | **yes** (rederive) | Learned-corrector research | Stage-5 residual corrector | model bytes (scored!) | decode CPU | Potentially high but unproven | Model-size Pareto; net-gain gate | PROPOSED |
| VOLE-Audio | Negative results recorded, literal wins reported (`docs/INVERSE.md` §9) | Losing hypotheses remain evidence | **yes** (discipline) | Experiment ledger | None | 0 | 0 | Improves search efficiency | Record all rejects | ADOPTED |

## 6. VOLE-Field — separation of state, observation, encoding

VOLE-Field is the unifying formalism: separate the state of a system, the observation
requested from it, and the physical encoding; `G_{t+1}=Φ_U(G_t,Δ_t)`,
`Ŷ_q=M_U(G_t,q)`, `Y*_q=Ŷ_q⊕_{ρ_q}R_q`, and a package is `F=(U,A,G_0,Δ,R,I,P)`
(`research/vole-field/vole-field-prior-art.md` §1,§3). It separates **semantic authority**
from **search capability**: arbitrary search may propose, but only validation produces a
canonical package (§4). Inverse compilation is multi-objective
`J(F)=(B_p,B_r,W_m,W_s,L,E,D)`, with a Pareto frontier preferred over one weighted scalar
(§5); residuals guide revision (§5.1); literal fallback is part of the architecture (§5.2).
It states the flattening tax `T_flat = J(C(M(F_native))) − J(F_native)` (§6), and lifts
EntropyFS’s `X=T(E(U,S,P))⊕R` from bytes to fields (§8). Its non-claims are unusually
relevant to Zentropy: no claim that structured state always beats conventional codecs, that
short seeds encode arbitrary high-entropy data, or that exact reconstruction implies
semantic truth (§35).

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| VOLE-Field | Separate system-state / observation / encoding (§1) | Three different things are routinely conflated | **yes** (discipline) | IR design framework | ZIR is an encoding, not the corpus | 0 | 0 | Prevents category errors | Adopt as IR design rule | ADOPTED |
| VOLE-Field | Search → bounded candidate → validation → canonical package (§4) | Search mechanisms vanish from the decode contract | **yes** (boundary) | Encoder search | Decoder is self-describing | 0 | 0 | Enables research/submission split | Enforce decoder independence | ADOPTED |
| VOLE-Field | Multi-objective `J(F)` + Pareto (§5) | Complete cost is a vector during research | partial | Experiment scoring | Scalar Hutter objective at submission | 0 | 0 | Honest trade-off analysis | Retain vector during research | PROPOSED |
| VOLE-Field | Residual-guided compilation loop (§5.1) | Residual structure indicates which family is failing | **yes** (loop) | DSFB-guided experiments | None | 0 | encoder-only | Medium–high | Implement residual-structure classifier | PROPOSED |
| VOLE-Field | Literal fallback is architecture, not failure (§5.2) | Bad models cannot damage the claim | **yes** | Negative controls | Universal `Literal` opcode | fallback bytes | cheap | Bounds worst case | Mandatory `Literal` candidate | ADOPTED |
| VOLE-Field | Flattening tax `T_flat` (§6) | Premature flattening is information-destructive | **yes** (metric) | Justifies source-native IR | Informs Phase 1 | 0 | 0 | Foundational | Measure on enwik | PROPOSED |
| VOLE-Field | `X=T(E(U,S,P))⊕R` lifted to fields (§8) | Same object algebra across modalities | **yes** (concept) | Unified candidate space | ZIR general object form | 0 | 0 | Conceptual unification | Apply to text objects | PROPOSED |
| VOLE-Field | Non-claim ledger (§35) | Explicit refusal of overclaims | **yes** (discipline) | Claim hygiene | None | 0 | 0 | Prevents disqualifying overclaims | Maintain non-claim list | ADOPTED |

## 7. FRF / frf-fuzz — evidence-first discipline

FRF is an evidence-control system: a mismatch is not noise until evidence says why, and a
claim may not cover a larger surface than the evidence that licenses it
(`research/Forensic Residual Framework/frf skill/frf-v2-SKILL.md`). The kernel is
`Authority → Court → Capture → Residual → Endoduction → Route → Disposition → Receipt →
Claim` (`research/Forensic Residual Framework/frf-main(5).zip:README.md`). Raw two-sided
observations are immutable; residuals get typed tokens via κ (`...:src/kappa.rs`); positive
claim prose is generated only by the claim compiler from receipt fields
(`...:src/sentences.rs`). Courts are falsified by **seeded mutation operators** and declared
external mutation providers: the extension proposes, the court decides
(`...:spec/mutation.md`, `...:src/mutation.rs`). Trajectories classify divergence as
`persistent/boundary-localized/version-stratified/gradual` over declared coordinate systems
(`...:src/trajectory.rs`). FRF ships a **deterministic seeded fuzz harness** under plain
`cargo test` (`...:tests/fuzz.rs`) plus corpus-guided libFuzzer targets (`...:fuzz/`). The
task’s four-verb loop (EXPLORE/AMPLIFY/DISCRIMINATE/FALSIFY) is **not present in any
inspected source** and a separate `frf-fuzz` snapshot does not exist; see §6 and §10 for the
honest mapping.

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| FRF | Immutable two-sided capture + receipt (`README.md`, `spec/openreceipt.md`) | Every result binds source revision, flags, hashes, sizes, time, RAM | **yes** (discipline) | Every experiment receipt | None | 0 scored (research disk only) | 0 | Prevents irreproducible results | Adopt receipt schema | ADOPTED |
| FRF | Court + admissibility envelope (`spec/evaluation.md`) | A court declares its scope up front; outside it returns UNKNOWN | **yes** | All mechanism courts | None | 0 | 0 | Prevents scope inflation | Define Zentropy courts (§Part II.6) | ADOPTED |
| FRF | Seeded mutation operators / challenge (`spec/mutation.md`, `src/mutation.rs`) | A passing court must *see* the defect class it polices | **yes** | Negative controls | None | 0 | extra test CPU | High (catches blind judges) | Challenge every exactness/score court | PROPOSED |
| FRF | Deterministic seeded fuzz harness (`tests/fuzz.rs`; `fuzz/`) | Reproducible hostile-input testing without nightly | **yes** (port harness pattern) | Archive/parser fuzzing | Decode-corruption court | 0 | fuzz CPU | Prevents decoder corruption | Fuzz ZIR parser, varints, ranks | PROPOSED |
| FRF | κ endoduction: deterministic residual→token table (`src/kappa.rs`) | Classification is a table, not a model | **yes** (rederive) | Residual routing | None | 0 | cheap | Routes experiments efficiently | Build residual-token table for text | PROPOSED |
| FRF | Residual trajectory classification (`src/trajectory.rs`) | Divergence has shape: persistent/localized/stratified/gradual | **yes** (rederive) | Regression/regime detection | None | 0 | cheap | Detects when a mechanism regresses | Track ΔS trajectories | PROPOSED |
| FRF | Claim compiler / non-claims (`src/sentences.rs`, `frf-quick-reference.md`) | Prose is generated from evidence, never hand-written | **yes** (discipline) | Report generation | None | 0 | 0 | Prevents overstated results | Auto-generate run reports | ADOPTED |
| frf-fuzz | Hypothesis-space search loop EXPLORE/AMPLIFY/DISCRIMINATE/FALSIFY | Search the hypothesis space, don’t mutate blindly | **cannot verify** | Intended mutation framework | None | 0 | encoder CPU | Unknown without source | **UNRESOLVED_REFERENCE** (see §10) | REJECTED (until grounded) |

## 8. DSFB — residual state observer

DSFB is a deterministic, trust-adaptive observer over multi-channel residuals. The core crate
implements a predict/correct loop over state `(φ, ω, α)` (position/drift/slew): predict
`φ⁻ = φ + ω·dt`, `ω⁻ = ω + α·dt`; residual `r_k = y_k − h(φ⁻)`; EMA
`s_k ← ρ·s_k + (1−ρ)·|r_k|`; raw trust `w̃_k = 1/(σ0 + s_k)` normalized to `w_k`; aggregate
`R = Σ w_k·r_k`; correction `φ ← φ⁻ + k_φ·R`, `ω ← ω⁻ + k_ω·R`, `α ← α⁻ + k_α·R`
(`research/dsfb/dsfb-main.zip:crates/dsfb/src/{observer,params,state,trust}.rs`). DSFB is
pure `f64`, deterministic, no ML, and in every downstream system it has **zero decoding
authority**: it may rank candidate families and set search budgets, but exact cost decides
the winner (`...:crates/dsfb/README.md`; EntropyFS `docs/adr/0004-dsfb-observer.md`).
EntropyFS’s selection wiring maps DSFB channels to candidate predictor families and derives
a bounded evidence scalar `y_k = clamp01(1 − log2(1+residual_cost_k)/log2(1+raw_cost))`
(`research/entropyfs/entropyfs-main(6).zip:docs/theory/dsfb-selection.md`). VOLE-Field notes
the measured caveat: DSFB’s marginal value diminished as the base representation floor
improved, so it must keep earning its place
(`research/vole-field/vole-field-prior-art.md` §9.1).

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| DSFB | `(φ, ω, α)` predict/correct with EMA trust (`crates/dsfb/src/observer.rs`, `trust.rs`) | Residual structure adapts search trust; deterministic, no ML | **yes** (rederive) | Residual observer for experiment selection | **none** (never in decoder) | 0 scored | O(M) per step | Search-cost only | Reimplement as a read-only observer | PROPOSED |
| DSFB | Zero decoding authority (`docs/adr/0004-dsfb-observer.md`) | Delete all DSFB state → decode unchanged | **yes** (invariant) | Governance only | None | 0 | 0 | Prevents correctness coupling | Decode-with-observer-deleted court | ADOPTED |
| DSFB | Channel evidence scalar `y_k` (`docs/theory/dsfb-selection.md`) | Bounded [0,1] evidence from exact residual cost | **yes** | Candidate family ranking | None | 0 | cheap | Medium | Ablate vs fixed/random ordering | PROPOSED |
| DSFB | Drift vs slew regimes (`docs/theory/dsfb-selection.md`, VOLE-Field §9) | Slow change → keep basis; abrupt change → broaden search | **yes** | Regime-driven experiment selection | None | 0 | cheap | Medium; measured marginal value shrinks as floor rises | Track marginal value, drop if negative | PROPOSED |
| DSFB | Residual trajectories as primary epistemic objects (framework docs) | The residual’s history is the evidence | **yes** (discipline) | Failure-mode clustering | None | 0 | 0 | Medium | Cluster articles by residual morphology | PROPOSED |
| DSFB | Augmentation, never replacement (`crates/dsfb/README.md`) | Read-only observer over existing methods | **yes** (discipline) | Research governance | None | 0 | 0 | Prevents architecture capture | Keep out of normative path | ADOPTED |

## 9. Gemel — evidence-native development memory (**snapshot absent**)

No `gemel` snapshot exists in `research/`. The only grounded description is in VOLE-Field,
which lists a “Gemel trajectory plane” carrying “intent, changes, rejected alternatives,
reconciliation, semantic lineage, evidence-bearing history” and states that “Gemel may
remember how a field evolved; it does not define how the field materializes”
(`research/vole-field/vole-field-prior-art.md` §Lines 1096, 1172–1180), citing
de Beer, *Gemel: Evidence-Native Version Control for Agentic Software Development*,
`gemel` 0.11.0, commit `4914c18b8178b572cb342338f065664f5ec2d97e`
(`research/vole-field/vole_field_prior_art.tex`, bib `ref119`). Because the engine’s schema
cannot be inspected, the mechanism is recorded as `UNRESOLVED_REFERENCE`; the intended field
list and the query-before-implement rule below come from the Zentropy brief and must not be
attributed to inspected source.

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| Gemel | Evidence-native development memory (VOLE-Field `ref119`) | Failed ideas are preserved, never deleted | **cannot verify** | Research experiment database | None | 0 scored | 0 | Prevents circular rediscovery | **UNRESOLVED_REFERENCE** (see §8, §10) | REJECTED (until grounded) |

## 10. ryg-rans-rs — the coding primitive

`ryg-rans-rs` is a native-Rust reconstruction of Fabian Giesen’s public-domain `ryg_rans`,
not a wrapper. It exposes four surfaces on one ANS state machine — byte rANS (u32 state, byte
renorm, `RANS_BYTE_L = 2^23`), R64 rANS (u64 state, u32 renorm, `RANS64_L = 2^31`), word rANS
(u32 state, u16 renorm, scale 12), and Vose alias byte rANS — with a division reference path
`C(s,x)=((x/freq)<<scale_bits)+(x%freq)+start` and an exact reciprocal fast path proven
equivalent by Kani
(`research/ryg-rans-rs/ryg-rans-rs-main.zip:crates/ryg-rans-rs-core/README.md`,
`docs/bitstream-contract.md`). It is `no_std`, `#![forbid(unsafe_code)]`, zero-allocation in
hot paths, and ships malformed-stream validators (`RenormGuard`, `validate_freq_model`,
`validate_byte_compressed`) that fail closed on hostile streams (`...:src/malformed.rs`). A
strict integrity contract, a bounded parallel block engine, exact backend semantics, and a
residual doctrine (“every diff is a first-class artifact until resolved”) are pinned by
receipts and ADRs. EntropyFS reuses it as a *thin adaptation layer* and forbids forking the
logic (`research/entropyfs/entropyfs-main(6).zip:docs/adr/0003-ryg-rans-rs.md`).

| source project | mechanism | semantic essence | code reuse appropriate? | research-plane role | submission-plane role | estimated byte cost | estimated runtime cost | expected information gain (EST) | experiment required | status |
|---|---|---|---|---|---|---|---|---|---|---|
| ryg-rans-rs | Byte rANS scalar + interleaved-2 (`crates/ryg-rans-rs-core`) | Table-based entropy coding with a pinned bitstream | **yes** (dependency or port with attribution) | Side-stream coder | rANS coder option | model + stream bytes | coder cost | High on static side streams | Coder-selection tournament | PROPOSED |
| ryg-rans-rs | Exact reciprocal fast path, Kani-proven (`docs/adr/0002`) | Division-free encode without changing the bitstream | **yes** | Performance only | Encode/decode speed | 0 | lower CPU | None (speed) | Benchmark judged path | PROPOSED |
| ryg-rans-rs | Malformed-stream validators (`src/malformed.rs`) | Corrupt streams fail closed, never panic/spin | **yes** (port rules) | Fuzzing | Decode-corruption court | negligible | validation cost | Prevents disqualifying crashes | Fuzz ZIR + coder boundaries | PROPOSED |
| ryg-rans-rs | Pinned bitstream contract (`docs/bitstream-contract.md`) | Ends, renormalization, flush, byte order are fixed | **yes** (port semantics) | Interop/oracle | Deterministic decode | 0 | 0 | Enables cross-checking | Cross-decode against upstream | PROPOSED |
| ryg-rans-rs | Strict decoded-hash integrity, bounded executor, atomic reorder (`docs/adr/0004`,`0006`,`0014`) | Parallel decode must be bit-exact and bounded | partial | Batch encode | Deterministic decode path | 0 | bounded | Safety | Determinism courts | PROPOSED |
| ryg-rans-rs | Residual primacy doctrine (`docs/residual-doctrine.md`) | Every diff recorded until classified | **yes** (discipline) | Research evidence | None | 0 | 0 | Improves correctness rate | Adopt residual records | ADOPTED |

---

# Part II — Dedicated sections

## 1. EntropyFS: configuration, grammar, rank, residual

**The object model.** EntropyFS persists a descriptor `D` such that `X = Materialize(D)`,
and its broadest form is the virtual entropy universe
`X = T(E_j(S,P,L), θ) ⊕ R` — a versioned deterministic generator `E_j` over seed/state `S`
and coordinate/configuration `P`, a bounded transform `T`, and an exact residual `R`
(`research/entropyfs/entropyfs_entropy_native_configurational_storage.tex` line 804;
`research/entropyfs/Entropyfs-Video-Descriptor.md` line 10 states the same form as
`X = T(E(U,S,P)) ⊕ R`). Specialized to enwik9, `E` is not a pseudo-random universe: it is
the set of *structured* universes whose coordinates describe data structure — sparse
popcount/positions/palette/permutation, periodicity, and grammar composition
(`...tex` §“Structured universes”). The specialized Zentropy object is therefore

```text
X = Reconstruct( Universe, CorpusIR, Grammar, Configuration, References,
                 PredictorState, Residual )
```

with every term charged (`docs/theory/information-accounting.md`).

**Rank/unrank.** The configurational families persist a coordinate instead of bytes:
`SPARSE` uses the combinatorial number system `rank = Σ C(p_i, i)` over marked positions;
`PALETTE` uses the multinomial rank `n!/∏c_i!`; `PERMUTATION` uses factoradic rank for
`m ≤ 34`; `PERIODIC` stores `(period, pattern, count, tail)`
(`research/entropyfs/entropyfs-main(6).zip:docs/theory/configurational-storage.md`,
`src/entropy/{rank,sparse,palette,permutation,periodic}.rs`). All arithmetic is checked
`u128`; a family whose state space overflows is *rejected*, never truncated. The invariant
`unrank(rank(x)) == x` and `rank(unrank(i)) == i` is property- and Kani-tested. The draft VVD
shows the same idea for a structured field: `MOTION_FIELD_RANK` stores the rank of a smooth
motion field rather than a thousand vectors (`research/entropyfs/Entropyfs-Video-Descriptor.md`).

**The object algebra for enwik9.** The transfer specializes the EntropyFS families to a text
object algebra that mirrors the brief’s ZIR:

| EntropyFS family | Zentropy specialization |
|---|---|
| `ZERO`/`FILL`/`PERIODIC` | constant whitespace/indent runs; repeated markup separators |
| `SPARSE` / `SPARSE_BLOCK64` | sparse edits over a base revision or template; sparse attribute positions |
| `PALETTE` | small alphabets: markup opcode streams, whitespace/case masks, link-target classes |
| `PERMUTATION` | permutations among article sections, template field order, list member order |
| `EXACT_REF` | `ExactReference`/`ArticleReference`/`Template` reuse |
| `BASE_RESIDUAL` | `DeltaSequence` over a prior article/template base |
| `GRAMMAR` (Phase 12D) | `GrammarProduction` + per-instance `RankedState` |
| `ENTROPY_REF` | `GeneratorState` (only with the random-XOF negative control) |
| `SEQUENCE_*` | `LongMatch`/`SparseMatch`/`RepeatedOffsetMatch` + entropy-coded streams |

**Phase 12D — what it actually found.** The 12D-0 offline oracle stored a bounded template
grammar (literal skeleton + slot positions) once and each member’s slot values raw, with
full accounting of grammar + state + residual + descriptor. On a grammar-friendly corpus it
reached **66,059 B (181.9×)**, losing to `zstd -19` whole-pack at **29,731 B (404.1×)** by
2.2× because the skeleton was stored literally; the diverse negative control showed the
grammar at 1.00× (no magic) (`...:docs/performance/grammar-oracle.md`). The 12D-1 round then
**entropy-coded the grammar skeleton** by running it through the store’s representation
search (byte-rANS, sequence-rANS, the four configurational families, and RAW, exact-cost
selected), giving `grammar_ec_total = chunk_cost(skeleton) + Σ(state + descriptor)`. The
skeleton’s 60,059 literal bytes became **29,156 B via SEQ_RANS at 3.88 bits/byte** plus
6,000 B state/descriptors, cutting the grammar’s total to **35,156 B (341.8×)** — a **−47%**
reduction — and closing the zstd gap from 2.2× to **1.18×**. The 12D line still **STOPPED**,
because the gate requires beating every incumbent, and zstd-whole remained 1.18× smaller
(`...:docs/performance/grammar-oracle.md`; `README.md` “12D grammar-addressed entropy
(STOPPED…)”). The document records the remaining 1.18× in exactly two places: **(a)
context-modeling quality** (order-1-style sequence modeling reaches 3.88 bits/byte where
zstd’s order-2+ reaches ~3.7; closing it needs a new order-2+ contextual coder) and **(b)
state encoding** (per-member fields stored raw, ~6,000 B / 17% of the grammar’s cost; rank
coding them would save ~2–3 KB, not enough alone). Both together land near the boundary.

**The five first-class Zentropy laws derived from 12D (ADOPTED).** These cost zero scored
bytes and are binding:

1. **Grammar itself has an information cost.** The grammar object is charged like any other
   descriptor; there is no “free skeleton.”
2. **Grammar must be entropy-coded.** The grammar object is data and runs through the same
   representation search as content (12D-1’s −47% is the receipt).
3. **State should be rank-coded when profitable.** Per-instance production state is a
   constrained vector and must be offered to the rank/unrank families; compare enumerative
   cost against entropy coding on actual emitted bits.
4. **Contextual coding quality can dominate the remaining gap.** On the 12D skeleton the
   residual gap was a context-order gap, not a grammar gap; the classical prediction floor is
   therefore a first-class mechanism, not a detail.
5. **Every layer must be fully accounted.** grammar + state + residual + descriptor + model;
   no layer may be reported separately as “the size.”

**Promotion gate.** `grammar_total_cost < incumbent_cost` after all five terms, exactly as
12D applied it. No exceptions.

## 2. VOLE: inverse proceduralization

VOLE’s inverse encoder is the conceptual core of the transfer
(`research/vole/vole-main.zip:docs/phase-g.md`; `examples/inverse_proof.rs`). The pipeline is:

```text
observation
  → candidate explanations (bounded, declarative)
  → materialize each through the normative reconstructor
  → exact residual closure R = target ⊖ prediction
  → complete-cost evaluation (all persisted bytes, including residual structure)
  → Pareto frontier / minimum complete cost
  → strictly smaller exact representation
```

Two disciplines make it trustworthy and are non-negotiable for Zentropy:

1. **Every candidate independently reconstructs its claimed byte interval exactly.** In
   Phase G, validity is established by materializing the candidate through the same
   normative primitives the decoder runs and comparing byte-for-byte with the target;
   winners are the minimum persisted-byte program, tie-broken by enumeration order. A
   candidate is never accepted because a detector “thinks” it fits
   (`...:docs/phase-g.md`).
2. **A candidate wins only on complete cost.** VOLE compares descriptor + object +
   checkpoint + transition + residual + model + state + dictionary + index + integrity
   bytes (`...:docs/information-accounting.md`; `docs/phase-o.md` §“every rewrite is
   accepted only when strictly smaller AND decode-identical”).

**Procedural text object families the transfer implies.** Each family below must
independently reconstruct its claimed byte interval exactly and must win only on complete
cost; those are admission requirements, not aspirations. The right-hand columns name the
certifying court and the EntropyFS/VOLE analogue that grounds the family.

| Candidate family | Claimed interval it must reconstruct exactly | Exactness court | Complete-cost gate | Grounding analogue |
|---|---|---|---|---|
| `Literal` | raw bytes | byte compare | always available; bounds worst case | VOLE `RAW`/`Literal`; EntropyFS `RAW` |
| `ExactReference` | byte range of an existing object | materialize reference + compare | reference bytes + descriptor vs literal | VOLE `EXACT_OBJECT_REF`; EntropyFS `EXACT_REF` |
| `DictionaryToken` | a dictionary entry’s bytes | table lookup + compare | index bits + table share | Brotli/EntropyFS dict (prior art, prompt §11.1) |
| `TransformedDictionaryToken` | entry under a declared reversible transform | inverse-transform + compare | index + transform tag + params | Brotli transforms; VOLE Phase-O generator substitution |
| `LongMatch` | copied interval (overlap allowed) | decoder-exact copy check | offset+length+stream bytes vs literal | EntropyFS `SEQUENCE_*`; Zstd/LZMA |
| `SparseMatch` | union of sparse matches | per-span compare | command stream accounting | EntropyFS `SPARSE_BLOCK64`; PAQ sparse |
| `RepeatedOffsetMatch` | copy at a recent distance | decoder-exact copy check | repcode symbol (no offset) | EntropyFS `SEQUENCE_DEEP` REP0/REP1 |
| `PhraseReference` | multi-token phrase | phrase table expansion + compare | phrase-id + table share | Brotli phrase dictionary |
| `ArticleReference` | whole article / section | article materialization + compare | id + delta vs literal region | VOLE Phase-O duplicate merge |
| `Template` | wiki template expansion | template render + compare | template def + field values | EntropyFS `TemplateGrammar`; VOLE generators |
| `GrammarProduction` | a produced span from `G, Θ` | production materialize + compare | grammar + state + residual | EntropyFS 12D |
| `MarkupProduction` | markup construct (`{{…}}`, `[[…]]`, `<tag>…</tag>`) | byte compare | construct id + args | VOLE `EMIT_TAG` analogue; prompt §12 |
| `LinkProduction` | `[[…]]` link with optional display span | byte compare | target + display delta | CMIX/link prior art (prompt §11.7) |
| `TableProduction` | table rows/columns | row-schema render + compare | schema + cell deltas | EntropyFS VVD sketches; CMIX tables |
| `ListProduction` | list shape + members | render + compare | shape + members | prompt §12 |
| `CitationProduction` | citation form | form render + compare | form id + fields | prompt §35 citation structures |
| `NumericProduction` | a number in a declared format | format render + compare | base + delta | VVD numeric/ID deltas; prompt §15 |
| `DateProduction` | a date/time value | calendar render + compare | base + delta | prompt §15 |
| `IdentifierProduction` | ISBN/DOI/ID-like strings | checksum/format render + compare | base + delta + checksum digits | prompt §15 |
| `TitleDerivedProduction` | a span derived from the page title | derive + compare | title id + transform | prompt §19 structural hoisting |
| `MorphologicalProduction` | surface form from root+affix | stem/suffix render + compare | root id + transform | Brotli stem transforms; prompt §11.1 |
| `CaseTransform` | case variant of a base | case apply + compare | mask/transform bits | Brotli capitalization transforms |
| `WhitespaceTransform` | whitespace variant | apply + compare | pattern bits | prompt §15 |
| `PunctuationTransform` | punctuation variant | apply + compare | pattern bits | prompt §15 |
| `Sequence` | an ordered op list | full decode + compare | op stream accounting | VOLE interval transitions |
| `DeltaSequence` | base + bounded edits | base apply + compare | BASE + residual buckets | EntropyFS `BASE_SEQUENCE`; VOLE Phase M |
| `RankedState` | a constrained configuration | `unrank(rank)==x` | `ceil(log2 N_states)` + descriptor | EntropyFS `SPARSE`/`PALETTE`/`PERMUTATION` |
| `EnumerativeState` | a multinomial/permutation state | same | exact state-space bits | EntropyFS `PALETTE`; enumerative coding |
| `GeneratorState` | program output | render + compare (byte-for-byte) | program + residual; random-XOF negative control | VOLE Phase N; EntropyFS `ENTROPY_REF` |
| `ResidualStream` | the exact difference under declared `ρ` | apply `⊕_ρ` + compare | residual + model + descriptor | VOLE `⊕_ρ`; EntropyFS residual buckets |

**Enforcement.** The admission API must require the two independent reconstructions VOLE-Audio
uses (`intrinsic closure` and `oracle observation`) before a candidate may enter the
frontier, and must distinguish **explanation** from **deduplication**: a shared reference
“does not explain the content, it points at stored content”
(`research/vole-audio/vole-audio-main(2).zip:docs/INVERSE.md` §6).

## 3. VOLE-Camera: do not flatten first

VOLE-Camera’s research question is whether starting the representation at the sensor
preserves useful deterministic state that is lost when observations are first flattened into
conventional raster/video frames (`research/vole-camera/vole-camera-main.zip:docs/non-claims.md`
§0). It measures two paths separately and never conflates them: **source-native** (known
state → native ingest → recording) and **flattened-inverse** (state → observations → discard
state → inverse factorizer → recording), and reports the flattening tax
`T_flatten = B_inverse / B_native` with all four byte counts
(`...:docs/non-claims.md` §“Source-native vs inverse”). It also insists that boundary
representations (e.g. RAW10 packing) are not canonical identity (`...:src/raw.rs`).

**Applied to enwik9: a source-native reversible Wikipedia IR.** Do not treat enwik9 as an
undifferentiated gigabyte and only later rediscover structure. Build a source-native
reversible IR that identifies, *without losing bytes*: document, page, title, namespace and
metadata, revision, timestamp-like fields, contributor fields, comments, text body, XML
structure, wiki markup, links, templates, tables, lists, references, HTML/XML tags, entities,
plain-language spans, code/pre/math-like spans, numbers, dates, identifiers, punctuation,
whitespace, and unknown/raw spans. Rules that come directly from the inspected sources:

- **Every parse operation must be invertible**; unknown or malformed content falls back to a
  literal span, never an irreversible normalization
  (`...:docs/non-claims.md`; VOLE-Field §1).
- **Boundary encodings are not canonical identity**; the IR must be byte-restorable and
  canonical-form verified (`...:src/raw.rs`; VOLE-GFX `docs/IR.md`).
- **The IR is valuable only if typed factorization reduces total description length**, not
  because it is semantically pretty (prompt §8; VOLE-Field §6).
- **Record `T_flatten`**: compare the source-native IR against a flattened-bytes baseline on
  enwik slices; if the IR does not reduce complete cost downstream, merge the stream back
  (`T_flatten` ≥ 1).
- **Residual-dominant outcomes are results**, recorded, not hidden
  (`...:docs/non-claims.md` §“The residual is not failure”).

**Status.** The snapshot implements only Phases A–B (canonical types, CFA, timing, raw
adapters). The factorizer/materializer/playback modules are absent, so their measured
behaviour is **UNRESOLVED_REFERENCE**; only the architecture, non-claims, and boundary
adapters are used above.

## 4. VOLE-GFX: search the explanation space

VOLE-GFX’s inverse compiler `A → (Γ, s, θ, R)` embodies the separation Zentropy must
preserve: **candidate generation is not semantic authority**. Detectors propose explanations
from cheap invariants; evaluation materializes each proposal and measures its exact residual;
a Pareto frontier admits non-dominated candidates
(`research/vole-gfx/vole-gfx-main(1).zip:docs/INVERSE_COMPILER.md`). Two operational details
transfer directly:

- **Host-independent acceptance.** The seeded-field predicate accepts a seed only if the
  field equals the surface on *every* sample; SIMD is a pure pruning/wall-clock device whose
  accepted set must equal the scalar oracle’s
  (`...:src/inverse/search.rs`). Zentropy’s GPU/AVX candidate screening may prune, but may
  never accept or reject: only the scalar exact reconstructor admits.
- **Search work is counted, not estimated.** `W_search = W_discovery + W_discrimination`,
  with each field incremented exactly where the operation occurs, and the literal fallback
  carrying zero search cost (`...:docs/INVERSE_COMPILER.md`). This makes “search found a
  better candidate” an auditable statement.
- **Counted-state overfit guard.** “No generator ‘explains’ pixels unless exact
  reconstruction follows from counted deterministic state”
  (`...:docs/NON_CLAIMS.md`). The analogue: no grammar/dictionary/template may be credited
  unless its definition + state + residual reconstruct the span exactly and are all charged.

**Application to Zentropy.** The research plane may run bounded parallel searches over
grammar candidates, dictionary transforms, match structures, article orders, context
combinations/hash sizes, state tables, mixer topology, entropy models, learned-model sizes
and quantizations, rank encodings, and stream partitioning — all of it, as long as only the
scalar exact reconstructor can admit a result. This is the mechanical basis for the
research-plane/submission-plane split.

## 5. VOLE-Audio: explanation + innovation

VOLE-Audio’s corrected architecture is the cleanest statement of the transferable principle:

```text
observation = deterministic explanation + entropy-coded innovation
```

“Store the deterministic explanation; entropy-code what the explanation cannot reproduce;
materialize sample-domain observations only when actually required”
(`research/vole-audio/vole-audio-main(2).zip:docs/ENTROPY_NATIVE.md`). Critically, entropy
coding is **orthogonal to the hypothesis family** — a representation may be literal,
literal-entropy-coded, procedural, procedural+residual, referenced, predictor+residual, or a
learned hypothesis + residual; “rANS object” is not itself a semantics
(`...:docs/ENTROPY_NATIVE.md`).

**The residual-conditioned learned corrector.** The Phase-O addendum (a written plan,
not implemented code) makes a compact learned deterministic model *another candidate family*,
judged by the same complete-cost API, with residual-aware training, deterministic
integer/fixed-point inference, SIMD/GPU parity, and scalar semantic authority unchanged
(`research/vole-audio/phase O.txt`). Mapped to Zentropy, this is the flagship experiment:

```text
Wikipedia-native deterministic model
  + classical probabilistic experts (PPM/context maps/word/stem/structural/match)
  → base probability / expected-symbol field
  → small learned residual corrector (trained on Stage-4 residual error, not raw bytes)
  → calibrated final probability
  → arithmetic/range coder
```

Admission rules, taken verbatim in spirit from VOLE-Audio and the brief:

- the learned hypothesis is **one more candidate family**, never semantic authority;
- acceptance requires exact reconstruction, and every candidate reports complete cost
  (`metadata + hypothesis + model + payload + index + checkpoint + dependency + integrity`);
- the net-gain gate is `residual_stream_bytes_saved − model_bytes − extra_binary_bytes > 0`,
  plus runtime and RAM gates;
- `RANS`/rANS is chosen only when `complete_rans_bytes < complete_raw_bytes`;
- recorded negative results (literal wins) are results.

## 6. FRF / frf-fuzz: evidence-first discipline and hypothesis-space search

FRF is the transfer’s scientific-integrity layer. Its hard laws, verbatim in intent from the
inspected skill (`research/Forensic Residual Framework/frf skill/frf-v2-SKILL.md`):

1. **Authority before claim.** Admit the witness (oracle/reference/contract/corpus) that can
   license the question.
2. **Question before implementation.** Prefer oracle-first experiments.
3. **Raw before interpretation.** Preserve two-sided raw observations before normalization.
4. **Residual before bug label.** A mismatch is an observation, not a diagnosis.
5. **Unknown stays Unknown.** A typed Unknown, not a pass/skip/flaky.
6. **Normalization weakens claims.** Every removed distinction leaves the claim surface.
7. **A pass must be sensitive.** Important courts need a mutation/negative control.
8. **Disposition is not evidence.** Closures require an evidence edge.
9. **History is monotone.** A later pass never erases an earlier unresolved residual.
10. **Claims are compiled.** Prose derives from verified evidence scope.

**Zentropy courts** (from the brief, grounded in FRF’s kernel): exact-reconstruction,
score-accounting, cross-build determinism, fresh-environment, resource-limit, runtime,
memory, disk, decoder-corruption, transform-reversibility, candidate-differential, ablation,
negative-control, and submission-packaging. Every result receipt binds source revision,
compiler/flags, input hash, binary/archive/decoded hashes, sizes, wall/CPU time, peak RSS,
temp disk, and environment (`...:README.md`; `spec/openreceipt.md`). Failed receipts are never
overwritten.

**Residual-guided hypothesis search, and the four-verb loop.** The brief asks for an
EXPLORE/AMPLIFY/DISCRIMINATE/FALSIFY architecture “adapted from frf-fuzz.” No inspected
source contains those verbs, and **there is no frf-fuzz snapshot**; therefore the four-verb
loop itself is recorded as `UNRESOLVED_REFERENCE` (§10). What *is* grounded and can be
adopted:

- **Deterministic mutation operators over configuration.** FRF’s mutation operators alter
  exactly one observable dimension and require a divergence on the targeted axis and only it
  (`FRF ...zip:spec/mutation.md`); `src/mutation.rs` implements the deterministic wrappers.
  Zentropy mutation operators (add/remove context, change order/hash width/table size,
  add/remove grammar production, split/merge stream, alter dictionary transform, change
  matcher depth/gap, change mixer feature/quantization, prune learned head, change rank
  family, change segmentation boundary) must each be deterministic and receipted.
- **Hypothesis-space search via seeded mutants.** FRF’s `frf court challenge` runs the court
  against a deterministic mutant and requires specificity-clean divergence; external mutation
  providers may propose but the court decides (`...:spec/mutation.md`,
  `...:README.md:357–363`). The analogous Zentropy discipline is boundary minimization:
  answer “what is the smallest model width / minimum context memory / dictionary size where
  the gain appears or turns negative?”
- **Deterministic fuzzing.** FRF ships a seeded in-repo harness that runs under plain
  `cargo test` (`FRF_FUZZ_ITERS`, default 20,000) plus libFuzzer targets with checked-in seed
  corpora (`...:tests/fuzz.rs`, `...:fuzz/`). Zentropy ports this pattern to the ZIR parser,
  varints, lengths, offsets, ranks, grammar/dictionary indices, model descriptions, and the
  arithmetic decoder.
- **Do not mutate blindly.** FRF’s residual-guided routing (κ) sends a residual to the next
  court or blocker rather than a random probe (`...:src/kappa.rs`); Zentropy’s mutation
  choice must be guided by the residual structure (DSFB, §7) and by Gemel’s record of prior
  attempts (§8).

An inference — explicitly *not* source-grounded — is that the intended four verbs map onto
these grounded pieces as: EXPLORE ≈ deterministic candidate/proposal search; AMPLIFY ≈ seeded
mutation over one declared axis; DISCRIMINATE ≈ specificity-clean challenge; FALSIFY ≈
negative-control/falsification courts. Until a frf-fuzz source is supplied, Zentropy uses the
grounded pieces above rather than inventing the loop.

## 7. DSFB: residual semiotics for search, not magic compression

DSFB has **zero authority over decoded bytes**. In EntropyFS the invariant is explicit: DSFB
may rank candidate predictor families, recognize regimes, and set search breadth; the winning
representation is always selected by exact deterministic cost; a filesystem image decodes
identically with all DSFB state deleted
(`research/entropyfs/entropyfs-main(6).zip:docs/adr/0004-dsfb-observer.md`). VOLE-Field states
the same boundary and adds the measured caveat that DSFB’s marginal benefit diminished as the
base representation floor improved, so it must keep earning its place
(`research/vole-field/vole-field-prior-art.md` §9.1).

**Residual streams to define** (each is a deterministic scalar trajectory over positions of
the corpus, with no decode authority):

| Residual stream | Definition | Proposed regime signal |
|---|---|---|
| `surprisal_t` | `−log2 p(actual_t)` from the current ensemble | high-entropy literal regions; model misses |
| `calibration_error_t` | `p̄_t − I[actual]` or Brier-like residual after the calibrator | mixer/SSE miscalibration |
| `model_disagreement_t` | variance/spread of expert predictions at `t` | regime boundary, unstable context |
| `match_failure_t` | `1 −` match length / expected match | missing or stale match model |
| `grammar_escape_t` | `1` when a span cannot be covered by any grammar production | missing grammar family |
| `literal_fallback_t` | `1` when `Literal` is the winning candidate | high-entropy/code/math/foreign spans |
| `rank_inefficiency_t` | `literal_bits − rank_bits` for the winning configuration | mis-modeled constrained state |
| `local_delta_bits_t` | `bits_t − bits_{t−1}` vs the baseline | local drift/slew events |

**Drift/slew semantics adapted from the core observer.** The grounded DSFB update is
`r_k = y_k − h(φ⁻)`, `s_k ← ρ s_k + (1−ρ)|r_k|`, `w̃_k = 1/(σ0+s_k)`,
`R = Σ w_k r_k`, with `φ,ω,α` corrections scaled by `k_φ,k_ω,k_α`
(`research/dsfb/dsfb-main.zip:crates/dsfb/src/observer.rs`, `trust.rs`). Applied to search
governance: small `|ω|`/`|α|` and low residual EMA ⇒ the current explanation family still
fits, narrow the search and keep the basis; large `|α|` or a residual-EMA jump ⇒ a regime
broke, reduce trust in the previous winner and broaden the candidate search
(`research/entropyfs/...:docs/theory/dsfb-selection.md`).

**Regimes DSFB can detect** and the experiments they propose: markup-heavy, ordinary prose,
tables, lists, numeric-heavy, template-heavy, citations, code, math, foreign-language,
escaped text, proper-name-dense, and high-entropy literal. A regime proposes **where to
investigate** — model switches, segmentation boundaries, missing contexts/transforms, grammar
families, dictionary families, article-clustering changes. **Only actual compressed-byte
reduction promotes a mechanism.** DSFB state is never persisted in the authoritative graph.

## 8. Gemel: never forget a failed idea

**Grounding caveat.** No Gemel snapshot exists in `research/`; only VOLE-Field’s description
(“trajectory and evidence-native version-control plane for field evolution, review, and
reconciliation”; “Gemel may remember how a field evolved; it does not define how the field
materializes”) and its citation to `gemel` 0.11.0, commit `4914c18b…`
(`research/vole-field/vole-field-prior-art.md` Lines 1096, 1172–1180;
`research/vole-field/vole_field_prior_art.tex` bib `ref119`). The exact schema is therefore
`UNRESOLVED_REFERENCE`. The intended role — from the Zentropy brief and the VOLE-Field
description — is an evidence-native development memory in which failed experiments are
first-class and never deleted.

**Fields to preserve per significant experiment** (brief §23; to be reconciled with the
eventual Gemel schema):

```text
intent; parent state; code revision; configuration; corpus hash;
compiler identity; machine; result; claim; evidence; residual;
reason adopted/rejected; follow-up
```

**Query-before-implement.** Before implementing an idea, query the memory: Has this already
been tried? Under what configuration? Why did it lose? Did later architecture changes
invalidate the old result? Compression research has enormous potential for circular
rediscovery; combined with FRF’s monotone history law (a later pass never erases an earlier
unresolved residual) and DSFB’s residual signatures, this closes the loop between search,
evidence, and memory. One concrete mechanism that *is* grounded independently and should
feed Gemel: ryg-rans-rs’s residual doctrine — every observed diff is a first-class artifact,
records are never deleted, and resolution requires classification
(`research/ryg-rans-rs/ryg-rans-rs-main.zip:docs/residual-doctrine.md`).

## 9. ryg-rans-rs

**What it provides.** A deterministic, `no_std`, `forbid(unsafe_code)` rANS core with four
surfaces (byte, R64, word, Vose alias), a division reference path and an exact reciprocal
fast path (Kani-proven equal), two-state byte interleaving, a table-based word decoder, and a
malformed-stream validation module that fails closed
(`research/ryg-rans-rs/ryg-rans-rs-main.zip:crates/ryg-rans-rs-core/README.md`,
`docs/bitstream-contract.md`, `src/malformed.rs`). It is not a wrapper — every operation is
implemented and cross-checked against upstream `ryg_rans`
(`...:docs/residual-doctrine.md`). EntropyFS uses it as a thin adaptation layer and forbids
forking it (`research/entropyfs/entropyfs-main(6).zip:docs/adr/0003-ryg-rans-rs.md`).

**Where rANS vs range coding is selected: by complete cost.** The selection rule is not
ideological:

- VOLE-Audio: choose `RANS` only when `complete_rans_bytes < complete_raw_bytes`, where the
  complete cost includes the block header, model bytes or model reference, rANS state, encoded
  symbols, indexes, alignment/padding, and integrity; uniform/incompressible data converging
  to RAW is a success condition
  (`research/vole-audio/vole-audio-main(2).zip:docs/ENTROPY_ACCOUNTING.md`).
- EntropyFS: a model wins only if `model_bytes + encoded_bytes + descriptor_bytes` beats every
  alternative (`...:docs/theory/rans-state.md`), under the explicit cost function
  (`...:docs/adr/0010-cost-function.md`).
- The brief’s guidance is consistent: prediction-heavy main text favors binary
  arithmetic/range coding; finite side streams (opcodes, lengths, offsets, masks, numeric
  deltas, dictionary indices) favor rANS/FSE; very small alphabets may favor specialized
  codes; a stream chooses its coder by actual emitted bytes including table descriptions.

**Zentropy policy (PROPOSED).** Implement a small set of coder primitives — `RAW`, binary
range/arithmetic (main adaptive text), rANS/FSE (static side streams), enumerative/rank
(constrained state), RLE/Golomb-Rice where apt — and select per stream by complete emitted
bytes, not by reputation. The adaptive range coder is expected to win on prediction-heavy
main text; rANS is expected to win on finite side streams. This must be measured, not
assumed. rANS is preferred over a bespoke range coder only where it wins on `S`; code reuse
from ryg-rans-rs (with attribution and the pinned bitstream contract) is appropriate, and
forking is not.

## 10. Explicitly unresolved references

| Reference | What is missing | What must be found before it is used |
|---|---|---|
| **Gemel** | No snapshot in `research/`. Only VOLE-Field’s description and citation (`gemel` 0.11.0, commit `4914c18b8178b572cb342338f065664f5ec2d97e`, `github.com/infinityabundance/gemel`) (`research/vole-field/vole_field_prior_art.tex` bib `ref119`). | The actual repository/engine at the pinned commit; its schema, query API, and event/evidence model. Until then §8 is the brief’s intent, not a grounded feature list. |
| **frf-fuzz** | No separate repository or archive. Referenced by VOLE-Field as “FRF-Fuzz … zero-authority exploration” (`research/vole-field/vole_field_prior_art.tex` Figure caption, line 1136). | The frf-fuzz source (or a statement that FRF’s own deterministic fuzz harness + mutation/challenge machinery *is* frf-fuzz). The four-verb EXPLORE/AMPLIFY/DISCRIMINATE/FALSIFY loop is **not present in any inspected source** and must not be attributed to FRF. |
| **DeSaxe** | Not a snapshot; name unresolved (per the brief, current discovery resolves only to an unrelated offset-crank engine concept). | The exact intended compressor/repository/paper. Do not attribute mechanisms. |
| **StarComp** | Not a snapshot; the identifiable project is an unrelated old SourceForge application. | The exact intended compressor/repository/paper. Do not attribute mechanisms. |
| **VOLE-Camera factorizer/materializer/playback** | Snapshot implements only Phases A–B; `src/` has no `sensor.rs`, `factor/`, `materialize.rs`, `residual.rs`, `presentation.rs`, `playback.rs`, `index.rs` despite `docs/architecture.md` listing them. | A later VOLE-Camera snapshot (or the prior-art paper’s measured factorizer courts) before citing any VOLE-Camera compression result. |
| **VOLE-GFX Phases L–N** | `docs/IMPLEMENTATION_STATE.md` marks Rayon/CUDA search, residual factoring, and DSFB “pending”; `docs/INVERSE_COMPILER.md` says “Phases L–N pending.” | Those phase receipts before citing GPU search or DSFB-governed inverse search as measured. |
| **VOLE-Audio Phase O (learned predictor)** | `phase O.txt` is a written addendum; `docs/INVERSE.md` §2 records delta/linear-predictor and harmonic families as *deferred* pending a universe amendment; `docs/ENTROPY_NATIVE.md` lists “(Phase O) learned hypothesis + entropy-coded residual” as planned. | An implemented Phase-O receipt. The residual-conditioned learned corrector in §5 is a PROPOSED design here, not an implemented/measured VOLE-Audio result. |
| **DSFB application crates** | The monorepo contains dozens of crates; this document read only the core `dsfb` crate and framework docs. | Any specific application crate (battery, chemical, GPU, etc.) before citing its mechanisms; none is needed for Zentropy. |

## 11. Transfer summary

Ranked by expected (Hutter-score gain) ÷ (engineering cost × uncertainty × resource risk),
with the concrete Phase where each candidate enters. All statuses are `PROPOSED` unless
marked; the cross-cutting laws are `ADOPTED` (zero scored bytes).

| Rank | Transfer candidate | Source grounding | Enters Phase | Byte cost (scored) | Uncertainty | Why ranked here |
|---|---|---|---|---|---|---|
| — | **Constitutional laws:** exact `decode(encode(X))==X`, universal `Literal` fallback, complete-cost accounting, search-has-no-decode-authority, entropy-code the grammar, rank-code state when profitable, literal fallback is a success | EntropyFS `grammar-oracle.md`, `information-accounting.md`, `adr/0004`; VOLE-Audio `INVERSE.md`; VOLE-Field §4–§5; FRF hard laws | Phase 0 | 0 | none | Free, blocking, and prevent invalid submissions. | 
| 1 | **Wikipedia-native structural hoisting + source-native reversible IR** | VOLE-Camera `non-claims.md`, `architecture.md`; VOLE-Field §6; VOLE `empirical-status.md` Phase Q flattening taxes | Phase 1 & Phase 3 | 0 direct; enables downstream | low | Everything downstream depends on a byte-restorable typed IR; flattening tax is measured in siblings; highest leverage per unit risk. |
| 2 | **Transformed lexical/phrase/procedural dictionary + match and repeat-offset state** | EntropyFS `SEQUENCE_DICT`/`SEQUENCE_SHARED_DICT`/`SEQUENCE_DEEP` (`adr/0005`); VOLE Phase-O generator substitution; Brotli/Zstd prior art (prompt §11.1–11.2) | Phase 4 | model + dictionary + reference bytes | low | Largest well-understood, low-uncertainty gain; reuse-by-reference and repeat-offset recency are proven primitives with cheap decode. |
| 3 | **Grammar + rank state with an entropy-coded grammar skeleton** | EntropyFS Phase 12D-0/12D-1 (`docs/performance/grammar-oracle.md`, `src/tests/grammar_oracle.rs`); VVD `MOTION_FIELD_RANK`; VOLE Phase N generators | Phase 5 | grammar + state + residual | medium–high | The only sibling *measured* transfer-relevant result: −47% on the grammar skeleton, gap reduced to 1.18×, with the remaining gap explicitly in (a) contextual modeling and (b) rank-coded state. High ceiling, real uncertainty. |
| 4 | **Article-layout compiler with equivalence-preserving representation rewriting** | VOLE Phase-O `docs/phase-o.md` (strictly-smaller + decode-identical rewrites); STARLIT lesson (prompt §11.11); VOLE `empirical-status.md` | Phase 7 & Phase 10 | ordering/restoration metadata (must be cheap) | medium | Offline search may be arbitrarily sophisticated while only a cheap reversible ordering survives; the rewrite-to-fixpoint pattern guarantees monotone size reduction. |
| 5 | **Residual-conditioned learned corrector** | VOLE-Audio `ENTROPY_NATIVE.md`, `phase O.txt`; prompt §34 cascade; current transformer frontier (prompt intro) | Phase 8 | **model bytes count against `S`** | high | Highest ceiling and hardest accounting; admitted only if it predicts the *residual* after cheaper stages and passes the net-gain gate `residual_saved − model_bytes − binary_bytes > 0` under runtime/RAM limits. |
| 6 | **DSFB residual semiotics as a search observer** | DSFB core `observer.rs`/`trust.rs`; EntropyFS `dsfb-selection.md`; VOLE-Field §9.1 | Phase 9 | 0 scored | medium | Reduces search cost only; measured marginal value shrinks as the floor rises, so it is explicitly droppable. |
| 7 | **FRF courts, receipts, mutation challenges, deterministic fuzzing, Gemel memory** | FRF kernel/`spec/mutation.md`/`tests/fuzz.rs`; ryg-rans-rs `residual-doctrine.md`; Gemel via VOLE-Field (unresolved) | Phase 0 & Phase 9 | 0 scored (research disk only) | low | Methodological infrastructure that keeps every other rank honest; Gemel portion is UNRESOLVED. |
| 8 | **rANS/FSE for finite side streams; range coding for prediction-heavy text; per-stream coder selection by complete cost** | ryg-rans-rs `README`/`bitstream-contract.md`/`malformed.rs`; EntropyFS `adr/0003`; VOLE-Audio `ENTROPY_ACCOUNTING.md` | Phase 2 & Phase 17-equivalent | coder + model bytes | low | Cheap, well-understood primitive with a firm selection rule; rANS is not privileged over range coding — emitted bytes decide. |
| 9 | **Rank/enumerative coding for constrained state** | EntropyFS `configurational-storage.md`, `src/entropy/*`; VOLE-Audio `frontier.rs` | Phase 5 & Phase 14-equivalent | `ceil(log2 N_states)` + descriptor | medium | High payoff on sparse/low-cardinality structured spans; must be compared against entropy coding on actual bits (12D’s rank-state gap was only ~2–3 KB on its corpus). |
| 10 | **Model/table reuse and content addressing** | EntropyFS `adr/0007`, `SEQUENCE_SHARED_DICT`; VOLE Phase P `empirical-status.md`; VOLE-Audio declared/unique/physical | Phase 6 & Phase 18-equivalent | reference + shared-object bytes | low–medium | Generalizes “state reuse is compression”; requires honest declared/unique/physical accounting so shared state is never reported as zero. |

**Milestone gates that decide the ranking empirically** (from the brief, mapped to the
courts above): G0 exact 1 GB reconstruction; G1 beats generic compressors; G2 competitive
with serious context mixing; G3 beats the historical Hutter record; G4 beats the strongest
credible pending frontier; G5 exceeds the 1% prize threshold against the applicable
predecessor; G6 5% safety margin; G7 full rule/resource closure. Each transfer candidate must
be ablated (`A0` floor through the cumulative ladder) before it is marked `MEASURED` or
`ADOPTED`, and a candidate outside Hutter resource limits is `RESEARCH_ONLY` regardless of
its byte savings.
