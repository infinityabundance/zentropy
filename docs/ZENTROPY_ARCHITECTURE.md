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
bytes ──► [ZIR-0 tokenisation]  (Phase 1, exact, measured)
      ──► [context models: orders 1,2,3,4,6,8 + match model]
      ──► [logistic mixer over expert predictions]
      ──► [APM/SSE calibration ×2]
      ──► [binary range coder]
      ──► archive9
```

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
| 0 | Rules, corpus, evidence constitution, score calculator, baseline harness | **MEASURED** (this repo) |
| 1 | Exact Wikipedia IR (ZIR-0) with RAW escape and full round-trip | **MEASURED** (exact on enwik6/8) |
| 2 | Minimal coding floor: range coder, rANS option, context model | **MEASURED** (enwik8 = 22,584,607 B) |
| 3 | Structural factorization, typed streams, structural hoisting | PROPOSED |
| 4 | Transformed lexical/phrase dictionary, long/sparse matches, repeat refs | PROPOSED |
| 5 | Procedural grammar + rank/enumerative state | PROPOSED |
| 6 | Serious context-mixing floor (ICM/ISSE, word/stem, SSE) | PARTIAL (direct models + word/bigram + match) |
| 7 | Article-layout compiler (semantic/structural/residual/predictor orders) | PROPOSED |
| 8 | Learned residual corrector (model-size Pareto campaign) | PROPOSED |
| 9 | Global search (DSFB observer, frf-fuzz mutation, Gemel memory) | PROPOSED |
| 10 | Equivalence-preserving representation optimizer | PROPOSED |
| 11 | Resource closure (RAM/CPU/disk/binary size/determinism) | PROPOSED |
| 12 | Submission closure (SFX, source, doc, receipts, licence, checklist) | PARTIAL (SFX stub + container) |

## 7. Measured results

`RawCm` is the Phase-2 floor: raw bytes through the context-mixing predictor, no
structural transform yet. All runs reconstruct exactly.

`RawCm` is the current floor: raw bytes through the context-mixing predictor
(orders 1,2,3,4,6,8 + word + word-bigram + match model), no structural transform
yet. All runs reconstruct exactly; each is bound to an immutable receipt in
`evidence/runs/receipts.jsonl`.

| Corpus | bytes | archive | bits/byte | ratio | wall (C+D) | peak RSS |
|---|---|---|---|---|---|---|
| selftest (synthetic) | 3,375,560 | 66,434 | 0.157 | 50.81 | — | — |
| enwik6 | 1,000,000 | 273,356 | 2.1868 | 3.66 | ~0.8 s | 22.8 MB |
| enwik7 | 10,000,000 | 2,485,288 | 1.9882 | 4.02 | 10.2 s | 76.0 MB |
| enwik8 | 100,000,000 | 22,465,931 | 1.7973 | 4.45 | 177.0 s | 447.0 MB |
| enwik9 | 1,000,000,000 | *see `evidence/runs/enwik9.report.txt`* | | | | |

Mechanisms admitted by measurement (each a sequential experiment; a mechanism
only counts when the *complete* `ΔS` is negative):

| Mechanism | measured ΔS | decision |
|---|---|---|
| word + word-bigram experts | −73,635 B on enwik7; −653,805 B on enwik8 | **ADOPTED** |
| orders 0, 5, 12, 16 added to the ladder | −12,551 B on enwik7; −118,676 B on enwik8 | **ADOPTED** |

Reference baselines measured on enwik8 by `tools/baseline.sh` (archive bytes):
`gzip -9` 36,445,248 · `bzip2 -9` 29,008,758 · `brotli -q 11` 25,742,001 ·
`zstd --ultra -22` 25,272,471 · `xz -9e` 24,831,656 · **zentropy RawCm
22,584,607**. The floor sits between `xz` and the PAQ lineage; the work of
Phases 3–8 is the climb to the frontier (`lpaq1` ≈ 1.98 bpc, `paq8` ≈ 1.44,
`cmix` ≈ 1.17, `fx2-cmix` ≈ 0.88).

### Submission-plane measurement

The scored stub (`target/submission/zentropy-sfx`, `opt-level="z"`, LTO,
stripped) is **313,792 bytes** and serves as both `comp9a` and `decomp9`. For
enwik6 it produces a 276,263-byte archive, so
`S = len(comp9a) + len(decomp9) + len(archive) = 590,055` bytes, and a
self-extracting `archive9` of 590,078 bytes reconstructs byte-identically with
no external inputs. These are genuine measured splits, not projections. The
stub is still far larger than a finished submission and is itself a Phase-11
optimisation target.

> **No claim of competitiveness is made yet.** The floor exists so that every
> subsequent mechanism can be attributed by ablation. A mechanism adds value
> only when the *complete* `ΔS` is negative.

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
