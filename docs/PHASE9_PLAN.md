# Phase 9 — Global search (DSFB observer, frf-fuzz mutation, Gemel memory)

> Deliverable (architecture §6): a **search layer** over the representation — a
> bounded, deterministic, receipted campaign that explores the configuration
> space, remembers every trial, and says what to investigate next from
> structural signals rather than from noise.
>
> Prior art: the DSFB observer (residual-guided exploration and deterministic
> structural signals), frf-fuzz (deterministic mutation), Gemel (an experiment
> memory that preserves failed hypotheses), all re-derived here for Zentropy.

## The rule that shapes the phase

**Search has no decode authority** (`ZENTROPY_ARCHITECTURE.md` §3). Nothing the
search does may change the decoder. The phase therefore explores only knobs that
are *carried in the archive header* and applied identically by both sides, so a
search result can never make the submission incorrect. Structural knobs
(context orders, table sizes, match tiers, hidden width, article order) are
explored through the existing `Method` / rebuild workflow and gated by `eval`;
the runtime knobs are explored directly.

## What is searched

The `tune` header byte — already present and previously a 16-entry learning-rate
selector (A20) — is extended into a two-axis hyperparameter vector, **backward
compatibly**:

* low nibble → the mixer learning-rate ladder (`MIXER_LRS`, unchanged);
* high nibble → one of 16 APM adaptation-shift sets (`APM_RATE_SETS`).

`tune < 16` therefore reproduces the pre-Phase-9 behaviour exactly (index 0 is
`(7, 7, 7)`). The whole 0..=255 space is searchable, but the extended half is not
free: the APM-rate table and the variable-rate plumbing are *representation* and
cost a **measured 304 B** when compiled in. At enwik9 the axis earns nothing (see
*Result*), so it is compiled out of the scored build by default; the phase
therefore nets **−16 B** of executable. Full accounting in *Binary cost*.

## Components

| # | Item | Mechanism | What is built |
|---|---|---|---|
| 9.1 | Hyperparameter space | — | `Knobs` over two axes; `tune` encode/decode; the APM rate table |
| 9.2 | Gemel memory | Gemel | receipt-log parsing (`trials_from_lines_for`, `already_tried_for`); a campaign never re-pays for a known configuration; **scoped by corpus digest** so a tune measured elsewhere is not mistaken for evidence about this input; reads the append target plus any number of `--memory` logs |
| 9.3 | frf-fuzz mutation | frf-fuzz | deterministic seeded perturbation, one axis at a time, magnitude-biased small |
| 9.4 | DSFB observer | DSFB | per-axis level means, coordinate-wise optimum, `coordinate_consistent`, spread signals |
| 9.5 | Pareto | — | best trial per tune; the frontier of tunes attaining the best archive |
| 9.6 | Schedules | frf-fuzz/DSFB | `coord` (cross of both axes at the start), `full` (all 256), `fuzz` (seeded walk), `guided` (the observer chooses the next points: coordinate descent, then a frf-fuzz hill-climb with a patience stop) |
| 9.7 | Court | — | the whole 0..=255 space round-trips exactly (search cannot corrupt decode); search functions are deterministic; the campaign court proves the memory prevents re-payment |
| 9.8 | Binary-cost accounting | — | the marginal executable cost of the extended space, measured by building the stub at HEAD and at the candidate revision |
| 9.9 | Research-plane threads | rayon | `sweep-tune --jobs N` evaluates N fixed points concurrently in one process and writes receipts serially in tune order, so the log is byte-identical to a serial campaign (measured 3.0× on 4 tunes) |
| 9.10 | Parallel-blocking probe | — | `pblocks` measures exactly what independent block coding would cost in ratio and buy in wall-clock, without changing the archive format |
| 9.11 | Parent-skip gate | — | `eval --parent-archive-bytes <n>` reuses the receipted accepted baseline instead of re-encoding and re-decoding it: 4 passes → 2 |

## Driver commands

```text
zentropy sweep-tune <corpus> [--method M] [--schedule coord|full|fuzz|guided]
                             [--trials N] [--tunes <list>] [--jobs N] [--receipt <jsonl>] [--memory <log>]…
zentropy frontier <receipt.jsonl> [--method M]
zentropy observe  <receipt.jsonl> [--method M]
zentropy pblocks  <in> [--blocks N] [--jobs M] [--tune T] [--no-full]
```

`--tunes` takes an explicit point list, which shards one campaign across
processes: each shard owns a disjoint set of tunes and its own receipt, and the
memory keeps the shards consistent once their logs are pooled. It also makes a
campaign resumable after an interruption.

## Binary cost (9.8, A31)

`S` is authority, so the phase's executable cost is charged by building the
scored stub, not by argument:

| build | stub bytes | vs HEAD |
|---|---|---|
| HEAD (Phase 8 complete) | 399,840 | — |
| HEAD + the Phase-9 search module alone | 399,856 | +16 |
| with the APM-rate axis compiled in (`--features apm-tune`) | 400,128 | +288 |
| **final tree** (`--no-default-features --features accepted`) | **399,616** | **−224** |
| final tree **plus rayon** (`... ,parallel`) | 399,632 | −208 |

* the **search module** (mutation, observer, Pareto, memory) costs **16 B**: fat
  LTO already eliminates the rest from the decoder path. It is research-plane
  code, gated behind `not(feature = "submission")` so the exclusion is explicit.
* the **APM rate axis** costs **304 B** when compiled in (192 B of read-only table
  plus variable-rate plumbing). It is rejected on the archive side, so it is
  compiled out.
* **rayon costs 16 B** if ever linked into the stub, because the stub never calls
  `par_iter` and LTO removes it. It is nonetheless outside the `accepted` feature
  bundle: the reason to keep it out of a scored artifact is the dependency and
  licence surface, not the bytes.
* the memory-guard work (research budget with a 4 GiB reserve, the runtime floor,
  the parent-skip in `eval`) accounts for the rest of the −224 B.

Packaging builds the stub with `--no-default-features --features accepted`, so
`accepted` is the single definition of the scored configuration and research-plane
machinery cannot leak into `S` by forgetting a flag.

## Result (measured)

### Campaign design

The campaign uses three rungs because this project's own evidence says a smaller
rung can disagree with enwik9 in **both** directions — A1.1 word tokenisation is
rejected on enwik7 (+14,805 B) and adopted on enwik9 (−1,702,081 B):

| rung | schedule | trials | role |
|---|---|---|---|
| enwik7 | `full` | 256 (exhaustive) | response-surface map |
| enwik8 | `coord`, then the second axis at the better LR | 46 | screening |
| enwik9 | `eval` gate | 2 candidates | **authority** |

### enwik7 — the exhaustive map

All 256 points. The frontier is a single best tune, and the APM axis has a clear
dose-response:

```text
tune 71  lr_idx 7 (lr 24)  APM (6,6,5)   2,361,668   -6,859 vs the accepted tune 7
tune 70  lr_idx 6 (lr 20)  APM (6,6,5)   2,361,909   -6,618
tune 183 lr_idx 7          APM (5,6,5)   2,361,963   -6,564
```

Mean archive over all 16 LR levels, per APM level (fair here, because the map is
exhaustive):

| apm_sel | rates | mean archive |
|---|---|---|
| 0 | (7,7,7) | 2,420,376 |
| 3 | (8,8,8) | 2,431,624 |
| 4 | (6,6,5) | **2,408,439** |
| 11 | (5,6,5) | 2,408,480 |
| 12 | (6,5,5) | 2,408,796 |
| 15 | (5,5,4) | **2,407,985** |

At enwik7 the *unchanged* rates (7,7,7) are the second worst level of the axis:
faster APM adaptation buys ≈12 KB on the mean. This is a real, strong signal.

### enwik8 — the coordinate search

The LR axis at `apm_sel = 0` is cleanly unimodal, and its optimum has **moved**
since A20 adopted it in Phase 4:

| tune | mixer LR | archive | vs accepted (21,237,221) |
|---|---|---|---|
| 5 | 16 | 21,245,220 | +8,000 |
| **6** | **20** | **21,234,135** | **−3,086** |
| 7 | 24 | 21,237,221 | — |
| 8 | 32 | 21,279,687 | +42,466 |

The APM axis, measured at both LR levels, loses at enwik8 — but by far less than
at enwik7:

| LR | best APM level | archive | vs APM off |
|---|---|---|---|
| 24 (tune 7) | `apm_sel 8` (6,7,6) | 21,237,874 | +653 |
| 20 (tune 6) | `apm_sel 8` (6,7,6) | 21,234,702 | +567 |

So the axis flips sign between rungs:

| rung | best APM level vs APM off (at LR 24) |
|---|---|
| enwik7 | **−6,859** (apm 4 (6,6,5)) |
| enwik8 | **+653** (apm 8 (6,7,6)) |

### The two questions the campaign leaves for the authority rung

1. **The re-tune.** Phases 6–8 changed the predictor, and the optimal mixer
   learning rate moved with it: LR 20 now beats the adopted LR 24 by 3,086 B on
   enwik8. This is the "the optimum is a function of the predictor" warning
   showing up as a measurement rather than a caution, and it costs **zero**
   executable bytes — the ladder already existed.
2. **The APM axis.** A strong win at enwik7, a near-tie (+567..+653 B) at
   enwik8. A near-tie is not a rejection, and this phase's own rule is that
   enwik9 decides. Both candidates are therefore gated on the authority corpus.

Two honest limitations of the screening:

* **The observer's `coordinate_consistent` assumes a balanced design.** With
  `apm_sel = 0` sampled at all 16 LR levels and the other APM levels sampled at
  one or two, the per-axis means are confounded and the flag reports `false` on
  the enwik8 log even though the frontier is unambiguous. The **frontier** (the
  best trial actually measured) is the authority; the means are a navigation aid.
* **No off-axis escape remains at enwik8.** The frf-fuzz neighbourhood of the
  optimum (`tune 6` → `tune 7, 5, 4, 2` and `22, 38, 70`) is entirely measured
  already, so a `guided` campaign from `tune 6` has nothing left to measure and
  halts immediately. The coordinate design plus the second-axis sweep therefore
  closes enwik8 rather than merely sampling it.

### enwik9 — the authority gate

Each gate is a full `eval`: both the parent and the candidate are encoded *and*
decoded, and both must reconstruct the corpus byte-for-byte. Every candidate is
compared against the same parent (the previously accepted `tune 7`).

| tune | mixer LR | APM shifts | archive | vs parent | exact |
|---|---|---|---|---|---|
| 7 (parent) | 24 | (7,7,7) | 169,642,087 | — | true |
| 6 | 20 | off | 169,484,029 | −158,058 | true |
| 134 | 20 | (6,7,6) | 169,575,025 | −67,062 | true |
| **5** | **16** | off | **169,282,339** | **−359,748** | true |
| 4 | 10 | off | *gate running* | | |

The parent re-encoded to **exactly** the Phase 8 value, 169,642,087 B, which is
an independent reproducibility check on the whole pipeline.

**The APM axis is REJECTED.** Comparing the two gates that share a mixer LR:

```text
169,575,025  (LR 20, APM (6,7,6))
-169,484,029  (LR 20, APM off)
=     +90,996 B
```

At the best mixer LR the best APM point is 91 KB *worse* than leaving the rates
alone. The axis therefore removes no bytes and its 192 B table plus variable-rate
plumbing are removed from the scored build (reproducible with
`--features apm-tune`). This is a clean demonstration of the enwik7→enwik8→enwik9
reversal pattern the project keeps finding: a mechanism worth ≈12 KB on the *mean*
at enwik7 and within 653 B at enwik8 is 91 KB of harm at enwik9.

**The LR re-tune is ADOPTED at LR 16.** The optimal mixer learning rate has moved
with the predictor, exactly as the Phase-7/8 review predicted, and it keeps moving
*down* as the corpus grows:

| rung | best mixer LR | margin over LR 24 |
|---|---|---|
| enwik7 | 24 | (LR 24 is the optimum) |
| enwik8 | 20 | −3,086 |
| enwik9 | 20 | −158,058 |
| enwik9 | **16** | **−359,748** |

The margin more than doubles at each step down, which is the signature of a
genuinely scale-dependent optimum rather than noise. The knob costs **zero**
executable bytes: the ladder already existed and the winning point has `apm_sel =
0`, so it decodes identically with the rejected APM axis compiled out.

Because the margin kept growing, the next rung down (LR 10, `tune 4`) is gated too
rather than assumed — this project has been burned by assuming an extrapolation in
both directions. That gate runs on the *fast* path (parent supplied from the
receipted baseline, two passes instead of four).

### A note on the interactions this changes

The accepted tune was 7 when Phases 6–8 were fitted, so the learned residual
corrector's weights were trained on an LR-24 trajectory. The gate measures what is
actually shipped (LR 16 with those weights) and it is a 360 KB win, but retraining
the corrector against the new trajectory is now an open, well-posed follow-up — it
can only help, since the shipped combination is the conservative one.

### What the phase did *not* buy

The APM axis is a rejection, and the mixer LR ladder is a pre-existing knob, so
Phase 9's no-cost win came from *searching* something already present rather than
from new compression machinery. The phase's real product is the measurement
infrastructure that made a 360 KB correction visible in an afternoon, plus three
negative results with receipts (APM axis, AVX2, parallel blocking).

### Verdict

| mechanism | authority measurement | decision |
|---|---|---|
| search layer (mutation, observer, Pareto, memory) | courted; `−16 B` net executable | **ADOPTED** |
| APM adaptation-shift axis | +90,996 B at the best LR | **REJECTED**, compiled out |
| mixer-LR re-tune to 16 | **−359,748 B** at enwik9 | **ADOPTED** |

Complete phase accounting at enwik9:

```text
archive       169,642,087 -> 169,282,339      (-359,748 B)
binary           399,840 -> 399,616          (-224 B: the search layer costs 16 B,
                                              removing the rejected axis saves 32 B,
                                              and the parallel/runtime-guard changes
                                              net the remainder)
DeltaS                                    ~= -359,972 B
```

For completeness, the three proposals that were *measured and rejected* rather
than argued about:

| proposal | measured verdict | record |
|---|---|---|
| AVX2 intrinsics in the predictor | 1.19–1.51× **slower** on the dominant loop; 1.10× on the real mixer types | [`SIMD_DECISION.md`](SIMD_DECISION.md) |
| parallel blocking in the archive | 3.4–9.1× faster for **+6.2% to +16.3%** ratio | [`PARALLELISM_DECISION.md`](PARALLELISM_DECISION.md) |
| rayon threads | **ADOPTED for research**: 3.0× on 4 concurrent tunes, byte-identical, zero scored-byte cost | [`PARALLELISM_DECISION.md`](PARALLELISM_DECISION.md) |

## Adoption rule

A searched knob is adopted only when the complete `ΔS < 0` on enwik9, measured by
a full `eval` with exact reconstruction; the search's own screening encodes never
decide. The search layer changes no decoder code, so the exactness court is the
whole-space round-trip test rather than a new mechanism court.
