# Zentropy Architecture

> Governing idea: **find the smallest executable explanation of `enwik9`.
> Persist the explanation, encode only the irreducible innovation, and charge
> every explanatory mechanism for every byte required to reconstruct the exact
> original.**

Zentropy is a new, independent, purpose-built Hutter Prize contender. It is
*inspired by* EntropyFS, FRF, frf-fuzz, Gemel, DSFB, VOLE, VOLE-GFX,
VOLE-Camera, VOLE-Audio and the wider VOLE stack, but it does not fork or wrap
any of them. Mechanisms are extracted, rederived, measured independently, and
admitted only if they lower the complete score.

## 1. The objective is description length

For every candidate mechanism `M`:

```
ΔS(M) = Δ payload_bytes + Δ decoder_binary_bytes + Δ compressor_binary_bytes
      + Δ frozen_model_bytes + Δ dictionary_bytes + Δ grammar_bytes
      + Δ transform_metadata + Δ restoration_metadata + Δ container_overhead
```

`M` is beneficial only if `ΔS(M) < 0` after exact reconstruction and resource
compliance. Bits-per-byte, compression ratio, payload size and model
cross-entropy are **diagnostics**, never authority. `S` is authority:

```
S = submitted_compressor_bytes + self_extracting_archive_bytes
```

## 2. Two planes

The single most important architectural decision is a hard separation:

| | Research / compiler plane | Submission plane |
|---|---|---|
| May use | threads, SIMD, AVX-512, CUDA/ROCm, huge RAM, offline search, grammar induction, clustering, learned models | only what is needed to produce and reconstruct the winning representation |
| Must be | arbitrary | deterministic, self-contained, compact, CPU-only, resource-bounded, exact, reproducible, byte-accounted |
| Knows about | FRF courts, Gemel, DSFB, experiments | **nothing** of it, unless a piece of that machinery itself lowers `S` |

The research stack discovers truth; the scored binary carries only profitable
truth.

## 3. Repository layout

```
src/
  corpus/      canonical corpus, provenance, SHA-256 (in-tree), ladder
  score/       S, gate, resource limits, eligibility  (single source of truth)
  evidence/    immutable run receipts, Gemel research memory
  entropy/     binary range coder, rANS side-stream backend
  mixer/       squash/stretch, adaptive mixer, APM/SSE calibration
  context/     direct context models, match model, composite predictor
  ir/          ZIR-0 exactly-reversible Wikipedia tokenisation
  archive/     the archive container (the scored payload)
  bin/         `zentropy` (driver) and `zentropy-sfx` (submission stub)
docs/          HUTTER_RULES, COMPETITIVE_BASELINE, PRIOR_ART_MECHANISMS,
               STACK_TECH_TRANSFER, ZENTROPY_ARCHITECTURE
evidence/      baseline digests, run receipts, courts, Gemel
experiments/   experiment definitions and results
fixtures/      micro fixtures for exactness courts
tools/         harnesses (baseline, courts, packaging)
research/      third-party reference material (gitignored, not redistributed)
```

The crate is a **single Rust package** with logical modules rather than a crate
graph, and **zero external dependencies** in the scored path. Rationale:
binary size is a first-class metric, determinism must be demonstrated, and the
licence inventory must be trivial. Even SHA-256 is implemented in-tree.

## 4. The pipeline (currently implemented)

```
bytes ──► [reversible structural hoist]      (Phase 3, fixed 31-entry table)
      ──► [reversible dynamic word tokens]   (Phase 4 / A1.1, ADOPTED)
      ──► [context models: orders 0..16, word, word-bigram,
           previous-line/column]
      ──► [match tiers: dense short, long-distance, sparse/gapped]
      ──► [matched-literal expert over the match prediction]  (Phase 4)
      ──► [logistic mixer over expert predictions]   (A20, re-tuned in Phase 9 to tune 5)
      ──► [APM/SSE calibration ×2]
      ──► [binary range coder]
      ──► archive9
```

Every transform carries a universal literal escape and is an exact bijection, so
the inverse chain reconstructs the original byte-for-byte.

> **ZIR-0 is implemented and exact but is NOT yet in the coding pipeline.** The
> source-native Wikipedia IR (`src/ir/`) currently backs only the `tokenize` and
> `selftest` commands. Wiring IR typing into the pipeline — typed streams and
> metadata/field hoisting — is the remaining Phase 3 work.

Decoding is the exact inverse with identical model state. The configuration is a
deterministic function of the declared output length, so it is not transmitted.

### Constituent modules

- **`entropy`** — carryless 32-bit binary range coder, exact and unit-tested at
  probability extremes; rANS with exact normalisation for side streams.
- **`mixer`** — `squash`/`stretch` logistic tables, a per-context adaptive
  logistic mixer (16.16 fixed point), and an APM/SSE interpolation stage.
- **`context`** — direct adaptive context models at several orders, a bitwise
  match model with collision verification, and the composite predictor.
- **`ir`** — ZIR-0, a partition of the input into typed spans (17 kinds) whose
  renderer is exact by construction, including on malformed input.
- **`archive`** — the container, with bounded expansion on corrupt input.
- **`corpus`/`score`/`evidence`** — Phase-0 governance: provenance, the objective
  function, and immutable receipts.

## 5. Constitutional laws

These are `ADOPTED` because they add zero scored bytes and block invalid
submissions:

1. **Exactness.** A mechanism that cannot round-trip exactly does not exist.
2. **Universal literal fallback.** Every parse/transform has a literal escape;
   unknown or malformed content never loses a byte.
3. **Complete-cost accounting.** Every mechanism is charged all its bytes,
   including model, metadata and restoration state.
4. **Search has no decode authority.** Accelerated search (SIMD/GPU/surrogate)
   may propose candidates; only the scalar exact reconstructor admits them.
5. **Entropy-code the grammar.** Any grammar/configuration description is
   itself compressed and charged.
6. **Rank-code state when profitable.** Finite constrained state is encoded as
   `rank(configuration)` when that beats literal or entropy coding.
7. **Delete negative-value experts.** An expert whose marginal `ΔS ≥ 0` is
   removed, regardless of its pedigree.

## 6. Phase plan and status

Honest status as of the current revision. `MEASURED` means the number exists in
`evidence/runs/`; everything else is `PROPOSED`.

| Phase | Deliverable | Status |
|---|---|---|
| 0 | Rules, corpus, evidence constitution, score calculator, baseline harness | **MEASURED** |
| 1 | Exact Wikipedia IR (ZIR-0) with RAW escape and full round-trip | **MEASURED** (exact on enwik6/8; **not yet in the pipeline**) |
| 2 | Minimal coding floor: range coder, rANS option, context model | **MEASURED** |
| 3 | Structural factorization, typed streams, structural hoisting | **COMPLETE (closed by measurement)** — fixed 31-entry hoisting ADOPTED and at saturation; IR-driven field hoisting and typed streams REJECTED at archive level (see §7.1) |
| 4 | Transformed lexical/phrase dictionary, long/sparse matches, repeat refs | **COMPLETE** — 255-word dictionary (A1.1) + long-distance and sparse match tiers + matched-literal expert adopted; repeat-offset state, distance floors, stemming, phrase/affix dictionaries and front-coding rejected with controls (see PHASE4_PLAN.md) |
| 5 | Procedural grammar + rank/enumerative state | **COMPLETE (closed by measurement)** — RePair/MR/one-shot grammars, first-use and rank-coded skeletons, LZBE factors all REJECTED; the loss grows with corpus size (see PHASE5_PLAN.md) |
| 6 | Serious context-mixing floor (ICM/ISSE, state maps, word/stem, SSE) | **COMPLETE** — the extra order-2 SSE stage (`sse-3`, enwik9 −1,671,235) is ADOPTED; state maps, ICM/ISSE, sparse contexts, collision control, the PPM-C expert, the stem model and high-order pruning are REJECTED with controls at enwik9/enwik8 (see PHASE6_PLAN.md) |
| 7 | Article-layout compiler (semantic/structural/residual/predictor orders) | **COMPLETE** — the encoder reorders `<page>` blocks by category set, template set, then title; the decoder restores the original order by a free sort on the embedded ascending page id. Adopted at enwik9: **−4,469,794 B** (174,533,527 → 170,063,733, 1.3605 bpc) for a measured 24,208 B. Content-similarity and greedy orders REJECTED; identity control exactly 0, shuffle control +25,519 (see PHASE7_PLAN.md) |
| 8 | Learned residual corrector (model-size Pareto campaign) | **COMPLETE** — a 120-byte quantized integer MLP consumes the classical mixer/APM outputs and emits a logit correction; offline-trained, embedded, charged. Adopted at enwik9: **−421,646 B** (170,063,733 → 169,642,087, 1.3571 bpc) for a measured 7,848 B; permuted-weight control +2,201,020. Net-gain gate passes (see PHASE8_PLAN.md) |
| 9 | Global search (DSFB observer, frf-fuzz mutation, Gemel memory) | **COMPLETE** — a deterministic, receipted search layer over the `tune` header byte (search has no decode authority). Campaigns: exhaustive on enwik7 (256 points), coordinate on enwik8 (46), gated by full `eval` on enwik9. The APM adaptation-shift axis is **REJECTED** at enwik9 (+90,996 B at the best LR) and compiled out; the search re-tuned the mixer learning rate 24 → 16 for **−359,748 B at zero binary cost**. The phase nets **−224 B** of executable. See §7.9 |
| 10 | Equivalence-preserving representation optimizer | PROPOSED — see [`PHASE10_PLAN.md`](PHASE10_PLAN.md) |
| 11 | Resource closure (RAM/CPU/disk/binary size/determinism) | **IN PROGRESS, two measured wins** — (a) the scored stub is **111,888 B**, down from 400,816 B by rebuilding `std` with `panic_abort`+`panic_immediate_abort`; since both legal forms charge the program twice that is **−577,856 B of `S`**, verified by a byte-identical archive; (b) `for_size` caps tables at 2^24 (576 MB for enwik9) against a 10 GB envelope and archive bytes fall monotonically as they grow (gates in flight). Also measured: the method dispatch is **not** the stub's cost (+24 B to narrow it, hypothesis withdrawn), `mem-guard` is 5,520 B, and the research half (`mem-floor`) is outside `accepted` and now **pauses** rather than aborting a long run. The T1 layout alternative is **REJECTED** ([`LAYOUT_DECISION.md`](LAYOUT_DECISION.md)). See [`RESOURCE_CLOSURE.md`](RESOURCE_CLOSURE.md), [`MEMORY_GUARD.md`](MEMORY_GUARD.md) |
| 12 | Submission closure (SFX, source, doc, receipts, licence, checklist) | **PARTIAL** — SFX stub + container + packaging court work; not yet a submission |

## 7. Measured results

The **accepted configuration** is `Method::Residual` (structural hoist + word token
reversion + column expert + Phase-4 match family + `sse-3` + article layout +
learned residual corrector) at **`tune 5`** (mixer LR 16, APM axis off). Every
number below reconstructs exactly and is bound to a receipt in `evidence/runs/`.

The accepted configuration is tuned **for enwik9, the scored corpus**. The smaller
rungs are screening instruments, and after Phase 9 they are slightly *worse* than
they were at the old tune — LR 16 costs enwik6/7/8 a few KB and buys enwik9 360 KB,
because the optimal mixer rate falls as the corpus grows (§7.9). Reading the ladder
as a monotone improvement is a category error: the target is enwik9.

| Corpus | bytes | archive (accepted) | bits/byte | ratio | encode wall | peak RSS |
|---|---|---|---|---|---|---|
| enwik6 | 1,000,000 | 267,333 | 2.1387 | 3.74 | ~1.0 s | — |
| enwik7 | 10,000,000 | 2,370,164 | 1.8961 | 4.22 | ~22 s | — |
| enwik8 | 100,000,000 | 21,245,220 | 1.6996 | 4.71 | ~245 s | — |
| enwik9 | 1,000,000,000 | 169,282,339 | 1.3543 | 5.91 | ~1,928 s | ~5.5 GiB |

The accepted configuration's executable is the **111,888 B** scored stub
(`--profile submission --no-default-features --features accepted`). `accepted` is
the single definition of the scored feature set, so research-plane machinery —
the Phase-9 search layer, rayon — cannot leak into `S` by forgetting a flag. Most
of the stub is dispatch for rejected methods, which Phase 11 (submission closure)
must reclaim; it is charged correctly here, but should not survive into a final
submission.

Mechanisms admitted by measurement (each a sequential experiment; a mechanism
only counts when the *complete* `ΔS` is negative):

| Mechanism | measured ΔS | decision |
|---|---|---|
| word + word-bigram experts | −73,635 B on enwik7; −653,805 B on enwik8 | **ADOPTED** |
| orders 0, 5, 12, 16 added to the ladder | −12,551 B on enwik7; −118,676 B on enwik8 | **ADOPTED** |
| structural hoisting (fixed dictionary in `.rodata`) | complete ΔS with **measured** 1,712 B executable cost: **+322 B on enwik6 (REJECTED)**, −4,472 B on enwik7, −15,130 B on enwik8 | **ADOPTED for ≥ enwik7** |
| A20 mixer learning rate 24 (tune 7, zero executable cost) | −9,492 B on enwik7; **−76,483 B on enwik8** — **superseded in Phase 9**: the optimum moved to LR 16 once Phases 6–8 landed (§7.9) | ADOPTED then, superseded now |
| A17 previous-line/column expert | −8,689 B on enwik7; **−58,737 B on enwik8** (measured 720 B cost) | **ADOPTED** |
| A1.1/A26 dynamic word tokenizer (corpus-derived dictionary in the archive, reverse ids) | complete ΔS with **measured** 21,848 B executable cost: **+14,805 B on enwik7 (REJECTED)**, −109,441 B on enwik8, **−1,702,081 B on enwik9** | **ADOPTED for large corpora** |
| Phase 4.1 long-distance match tier (Z6) | enwik7 −4,229; enwik8 −77,348; **enwik9 −1,454,881** (1,240 B cost) | **ADOPTED** |
| Phase 4.2 sparse/gapped match tier (C9) | enwik7 −5,636; enwik8 −30,993 | **ADOPTED** (composite) |
| Phase 4.4 matched-literal expert (X2/X3) | enwik7 −43,858; enwik8 −333,917 (control −1,280 / −12,219) | **ADOPTED** (composite) |
| Phase 4 composite (`phase4`, 4.1+4.2+4.4) | **enwik9 −3,869,340** at the fully accounted 5,576 B marginal | **ADOPTED** |
| Phase 6.4 order-2 SSE stage (`sse-3`) | enwik7 −14,897; enwik8 −195,864; **enwik9 −1,671,235** (256 B; control −305,341) | **ADOPTED** |
| Phase 6.8 bounded PPM-C expert (`ppm`) | enwik7 −49,505 over `sse-3`; enwik8 −121,466 over `sse-3`; **enwik9 +61,133 (REJECTED)** — fixed table capacity | **REJECTED at scale** |
| Phase 7 article-layout compiler (`reorder-full`) | enwik7 −21,722; enwik8 −317,313; **enwik9 −4,469,794** (24,208 B; identity control 0, shuffle control +25,519); **zero permutation bytes** paid | **ADOPTED** |
| Phase 8 learned residual corrector (`residual`, 120 B model) | enwik7 −7,831; enwik8 −65,245; **enwik9 −421,646** (7,848 B binary incl. weights; permuted-weight control +2,201,020) | **ADOPTED (accepted configuration)** |
| Phase 9 search layer (DSFB observer, frf-fuzz mutation, Pareto, corpus-scoped Gemel memory) | courted (whole 0..=255 space round-trips; memory prevents re-payment; campaigns deterministic) | **ADOPTED** (net **−224 B** of executable) |
| Phase 9 mixer-LR re-tune (**LR 16**, `tune 5`, zero executable cost) | enwik9 **−359,748** (169,642,087 → 169,282,339, 1.3543 bpc); LR 20 was −158,058 and LR 24 the old accepted value | **ADOPTED** |
| Phase 9 APM adaptation-shift axis | enwik7 ≈−12 KB on the mean, enwik8 +653 at the best LR, **enwik9 +90,996 at the best LR** | **REJECTED at scale**, compiled out (`--features apm-tune` reproduces it) |

> **Accounting note.** The executable cost of a mechanism is *measured*, never
> estimated: build an otherwise-identical submission binary with and without the
> `struct-hoist` feature (`tools/measure_binary_cost.sh`) and charge the delta.
> The first estimate for this mechanism was 543 B; the measured cost is 1,728 B,
> which is enough to flip the enwik6 verdict from ADOPTED to REJECTED. This is
> precisely the class of error the constitution exists to prevent.

The enwik9 run is the full-corpus milestone: exact 10⁹-byte reconstruction,
beating every generic compressor (`xz -9e` ≈ 197 MB, `bzip2 -9` ≈ 254 MB,
`gzip -9` ≈ 322 MB). It is ~1.53× above the accepted Hutter record (`fx2-cmix`,
110,793,128 B) — a gap of **58,489,211 B (~58.5 MB)** — and the peak is inside the
10 GB limit.

Reference baselines measured on enwik8 by `tools/baseline.sh` (archive bytes):
`gzip -9` 36,445,248 · `bzip2 -9` 29,008,758 · `brotli -q 11` 25,742,001 ·
`zstd --ultra -22` 25,272,471 · `xz -9e` 24,831,656 · **zentropy accepted
22,181,992**. The floor sits between `xz` and the PAQ lineage; the work of
Phases 3–8 is the climb to the frontier (`lpaq1` ≈ 1.98 bpc, `paq8` ≈ 1.44,
`cmix` ≈ 1.17, `fx2-cmix` ≈ 0.88).

### Phase 3 completion — structural modeling on the IR

Phase 3's remaining scope (`PRIOR_ART_MECHANISMS.md` ranks 8–9: reversible enwik
preprocessors, metadata/field hoisting, typed streams) was evaluated against the
accepted model on enwik7. All of it is closed by measurement.

**Tag census.** All XML tags total 261,442 B (2.6% of the corpus). The fixed
31-entry hoist covers 228,062 B (87%) by prefix; 33,380 B is uncovered. The
largest uncovered item is `<text xml:space="preserve">` (27 B x 1,326 = 34.5 KB
raw). Replacing it with a one-byte code changes the archive by **-309 B** — 34.5
KB of raw structure is worth 0.3 KB to the model. The code space is full
(`0x01..=0x1F` is 31 codes and `0x20` is a literal space), so extending the table
would require *dropping* an entry to gain ~300 B, within the binary cost of the
edit. Tag hoisting is saturated.

**Metadata census.** `sha1`, `parentid`, `ns`, `model`, `format` do not occur at
all — the enwik dump strips them. Present fields (contributor 118 KB, comment
62 KB, timestamp 57 KB, id 53 KB, title 39 KB) total ~3.5% of the corpus, and
98.7% of bytes live inside `<revision>`, i.e. article text. Neutralising the
contents of timestamp/id/username/ip/comment at equal length shrinks the enwik7
archive by **9,440 B (0.38%)** with mixed fills and **32,091 B (1.31%)** with a
uniform fill — the hard upper bound on any field production. Naive productions
make it *worse*: binary-packing timestamps to 7 B (raw -17,238 B) **increases**
the archive by 1,319 B, and varint-packing `<id>` (raw -9,839 B) increases it by
2,158 B. The CM models ASCII digit structure better than a compact binary field.

**Typed streams.** The prerequisite claim — that the model cannot see structure —
is false: the 309 B figure above shows the model already predicts structural
strings from prefix context. Explicit type markers would spend bytes carrying
information the model already has.

**Verdict.** Fixed structural hoisting is adopted and at saturation; IR-driven
field hoisting and typed streams are rejected at the archive level. enwik's
bytes are not in its structure (5–6% of bytes, and cheap for the CM); they are in
its article text (98.7%), which is Phase 4/6 work.

> **Method note.** These are archive-level *screenings* (a Python transform of
> the corpus plus the accepted `compress` path), not fully-packaged candidates.
> They are decision-grade for rejection: a real implementation inherits the same
transformed representation and can only add binary cost. A production that
> preserves the ASCII/logical form (rather than packing to binary) was not found;
> that is the one avenue left open, and the neutralisation bound says it is worth
> at most ~1.3% of the archive before its own cost.

### Submission-plane measurement

The accepted configuration is the `residual` method at `tune 5`. The scored stub
(`target/submission/zentropy-sfx`, `--profile submission --no-default-features
--features accepted`, `opt-level="z"`, LTO, stripped) is both `comp9a` and
`decomp9` and is **111,888 B**. Measured on enwik6 (`bhm = 267,333 B`,
`archive9 = 666,972 B`) the two legal packaging forms score:

```
S(self-extracting:  comp9 + archive9)          = 1,066,588
S(separate, comp9a = decomp9:  2P + bhm)       = 1,066,565
exactness: byte-identical
```

The two differ by exactly 23 bytes — the SFX marker (15) plus the length field
(8) — which is a useful sanity check that both accounting paths are consistent.
Note that the separate form charges the single program **twice** (the rule's
`2×decomp9` reduces to `1×decomp9`, leaving `comp9a + decomp9`). An earlier
revision of `tools/package_sfx.sh` printed `P + bhm` and undercounted by one
full copy of the program; this is now fixed and unit-tested in
[`score`](../src/score/mod.rs).

The stub is still far larger than a finished submission and is itself a Phase-11
optimisation target.

> **No claim of competitiveness is made yet.** The floor exists so that every
> subsequent mechanism can be attributed by ablation. A mechanism adds value
> only when the *complete* `ΔS` is negative.

### 7.9 Phase 9 completion — global search

The `tune` header byte became a two-axis hyperparameter vector: the low nibble
selects one of 16 mixer learning rates, the high nibble one of 16 APM
adaptation-shift sets. `tune < 16` reproduces the pre-Phase-9 behaviour exactly.
Around it sits the search layer: deterministic frf-fuzz mutation, a DSFB observer
(per-axis means, coordinate-wise optimum, `coordinate_consistent`, spread), a
Pareto report, and a Gemel memory that is **scoped by corpus digest** and can read
any number of receipt logs, so a campaign never re-pays for a configuration it has
already measured — or mistakes a tune measured on another corpus for evidence
about this one. **Search has no decode authority**: the knob is a header byte both
sides apply identically, so the exactness court is the whole 0..=255 space
round-tripping.

Campaigns: **exhaustive** on enwik7 (256 points), **coordinate** on enwik8 (46,
covering both axes at two LR levels), and **gated by full `eval` on enwik9**.

| mechanism | authority measurement (enwik9) | binary | decision |
|---|---|---|---|
| search layer | courted; campaigns deterministic; memory verified | +16 B | **ADOPTED** |
| APM adaptation-shift axis | +90,996 B at the best LR | +304 B | **REJECTED**, compiled out |
| mixer-LR re-tune 24 → 16 | **−359,748 B** | 0 B | **ADOPTED** |

The phase's headline result is that **the optimal mixer learning rate is a
function of the predictor and of scale**, and that A20 had tuned it against a
predictor that no longer exists:

| rung | best mixer LR | margin over LR 24 |
|---|---|---|
| enwik7 | 24 | — |
| enwik8 | 20 | −3,086 |
| enwik9 | 20 | −158,058 |
| enwik9 | **16** | **−359,748** |

Every step down the ladder bought more, so the rung below (LR 10) was gated rather
than extrapolated. The knob costs zero executable bytes because the ladder already
existed and the winning point has the APM axis off.

The APM axis is the phase's clean negative: worth ≈12 KB on the *mean* at enwik7
and within 653 B at enwik8, it is **91 KB of harm** at enwik9. A near-tie at a
smaller rung is not a rejection, which is exactly why the authority gate exists.

The rejected axis left the high nibble of `tune` free. T2 now uses it to carry a
**table-size scale** (`tune = (scale << 4) | lr_idx`), which is a model-geometry
knob rather than a coding one, and is decoder-derivable by construction because
both sides read the archive's own `tune` byte. `tune < 16` is bit-identical to the
pre-T2 behaviour, and `tune-table` + `apm-tune` is a `compile_error!` since they
claim the same nibble. Status: **measured, gates in flight, not adopted** — see
[`RESOURCE_CLOSURE.md`](RESOURCE_CLOSURE.md).

### 7.10 Throughput: two mechanisms measured and rejected

> The full records are [`THROUGHPUT_ANALYSIS.md`](THROUGHPUT_ANALYSIS.md) (the
> diagnosis), [`SIMD_DECISION.md`](SIMD_DECISION.md),
> [`PARALLELISM_DECISION.md`](PARALLELISM_DECISION.md) and
> [`LAYOUT_DECISION.md`](LAYOUT_DECISION.md).

Coding runs at ~2.4 µs/byte (~1000 cycles/bit), which reads like an emergency. It
is not one: ~1.5 core-hours per enwik9 pass against a ~53 core-hour allowance, so
**throughput cannot buy score**. What it does cost is research iterations, and the
levers that actually paid were the research-plane ones (rayon across candidates
3.0×; `--parent-archive-bytes` halves a gate; concurrency itself).

| mechanism | authority measurement | decision |
|---|---|---|
| AVX2 intrinsics in the predictor | 1.19–1.51× **slower** on the dominant loop; 1.10× only on the real mixer types | **REJECTED** |
| parallel blocking in the archive | 3.4–9.1× faster for **+6.2% to +16.3%** ratio | **REJECTED for the archive** |
| bucket-local (nibble) table layout | affordable subset **0.97×** for +7,071 B; ceiling 1.29×/1.39× for +66,674/+597,425 B | **REJECTED** |
| rayon across candidate tunes | **3.0×**, archives byte-identical | **ADOPTED** (research plane) |
| `--bits` working-set sweep | 1293 → 2500 ns/byte as the model grows 39 → 374 MB, probe count unchanged | diagnosis confirmed |

The `--bits` sweep is the one that settles the argument: at `bits = 24` enwik9 is
already on the **flat** part of the working-set curve, so there is no cheap
locality win left, and the layout that *would* reduce lines touched cannot be
afforded in ratio. The sweep's side effect was the opposite finding of real value:
bigger tables buy archive bytes, which is T2 in §7.9.

Three proposals were measured and rejected rather than argued about; both records
are in-tree so the questions are not re-litigated from intuition:

| proposal | measured verdict | record |
|---|---|---|
| AVX2 intrinsics in the predictor | 1.19–1.51× **slower** on the dominant (random table) loop; only 1.10× on the real mixer types | [`SIMD_DECISION.md`](SIMD_DECISION.md) |
| parallel blocking in the archive | 3.4–9.1× faster for **+6.2% to +16.3%** ratio — an order of magnitude worse than the phase's win | [`PARALLELISM_DECISION.md`](PARALLELISM_DECISION.md) |
| rayon research threads | **ADOPTED for the research plane**: 3.0× on four concurrent tunes, byte-identical output, ~16 B if ever linked into the stub | [`PARALLELISM_DECISION.md`](PARALLELISM_DECISION.md) |

And one pure-waste fix that mattered more than any of them: `eval` used to spend
**two of its four full passes** re-encoding and re-decoding the parent — the
already-accepted configuration whose archive size is receipted. `eval
--parent-archive-bytes <n>` skips those passes (the candidate is still encoded and
exactly decoded), halving every gate; `tools/gate_many.sh` then runs N gates
concurrently. A gate that took 2.5 hours now takes ~1 hour, and that is a
measurement-discipline change, not a compression result.

> **Open, well-posed follow-up.** The learned residual corrector's weights were
trained on an LR-24 trajectory. The gate measures what is actually shipped (LR 16
with those weights) and it is a 360 KB win, but retraining the corrector against
the new trajectory can only help.

## 8. Admission procedure

1. State the hypothesis and the expected source of saving.
2. Implement the exact mechanism behind the `Literal` fallback.
3. Run the exactness court on fixtures, malformed inputs, and corpus slices.
4. Run the ladder; record an immutable receipt with all resource numbers.
5. Run the negative control that destroys the signal the mechanism claims to
   exploit. If it performs the same, the explanation is probably wrong.
6. Ablate: measure `ΔS` against the parent, not against the world.
7. Admit, reject, or mark `RESEARCH_ONLY`.

## 9. Build and run

```sh
cargo test                       # exactness courts for every module
cargo build --release

./target/release/zentropy gate
./target/release/zentropy hash       evidence/corpus/enwik8
./target/release/zentropy tokenize   evidence/corpus/enwik6
./target/release/zentropy bench      evidence/corpus/enwik7
./target/release/zentropy verify     evidence/corpus/enwik8 evidence/runs/enwik8.rawcm.znt
./target/release/zentropy compress   <in> <archive>
./target/release/zentropy decompress <archive> <out>
```

The submission stub is built separately and packed by the Phase-12 tooling:
`zentropy-sfx` reads a marker + length + archive appended to its own image and
writes the reconstructed corpus with no external inputs.

## 10. What Zentropy must not become

A filesystem, a general archive format, a CMIX wrapper, a benchmark dashboard, a
collection of codecs, an ML demo, a GPU-only compressor, a framework with no
competitive artefact, a giant crate graph, or an enwik9 lookup table disguised
as compression. It is a focused compression research machine whose final product
is a valid Hutter submission.
