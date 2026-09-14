# Parallelism decision record — rayon, threads, and parallel blocking

> **Question.** Can Zentropy use the machine's other 15 cores to go faster?
>
> **Answer.** Yes across *independent work* (candidate trials), no across a
> *single pass* — and the only way to parallelise one pass, independent blocking,
> costs far more ratio than it is worth. Details and numbers below.
>
> Companion record: [`SIMD_DECISION.md`](SIMD_DECISION.md) (AVX2 is slower here).

## 1. What the rules permit

`HUTTER_RULES.md` §4, from the primary source: the rules publish **both**
single-core and multi-core `T` values (Intel 1427/4667, AMD 1310/8228), so
multi-threaded execution is anticipated rather than forbidden. What binds instead
is total RAM ≤ 10 GB, the wall-clock budget `70,000/T`, and bit-exactness.

Two constraints shape the design regardless:

* **RAM is total, not per-thread.** N concurrent models are N× the memory.
* **Determinism.** Our model is integer-only, so a fixed decomposition is
  reproducible; the risk is only in *unfixed* work-splitting (e.g. a reduction
  whose order depends on thread scheduling). We never do that: each parallel unit
  is a complete, independent encode whose result depends only on its input.

## 2. Threads across independent work — ADOPTED

`sweep-tune --jobs N` evaluates N *fixed* candidate points concurrently in one
process, sharing one loaded corpus. Receipts are written serially in tune order
after the parallel encodes finish, so the log is byte-identical to a serial run.

Measured on enwik7, four tunes (`5,6,7,8`):

| jobs | wall | speedup | archive bytes |
|---|---|---|---|
| 1 | 82.0 s | — | 2370164 / 2368538 / 2368527 / 2371196 |
| 4 | 26.0 s | **3.0×** | identical, tune-for-tune |

Correctness: every tune's archive is byte-identical between the serial and
parallel runs. The adaptive `guided` schedule stays serial because it chooses its
next point from measured results; `coord`/`full`/`fuzz`/`--tunes` are parallel.

**Scored-path cost: zero.** `rayon` is an *optional* dependency, not in
`accepted`, so the submission stub never links it. Measured anyway, for honesty
(`RUSTFLAGS="" cargo build --profile submission --no-default-features --features …`):

| features | stub bytes |
|---|---|
| `accepted` (the scored stub) | **399,632** |
| `accepted,parallel` | 399,616 |

The `parallel` build is **16 B smaller**, i.e. the difference is code-layout noise
from fat LTO, not a cost of rayon — nothing in the SFX path calls it. Treat ±16 B
as the layout-noise band for any single-feature measurement; do not read a 16 B
delta as a mechanism's price. The reason to keep rayon out is not bytes but the
dependency/licence surface of a scored artifact.

## 3. Parallel blocking — REJECTED for the archive, kept as a probe

Splitting the corpus into N independently-coded blocks parallelises *both* encode
and decode with no overlap, and per-block models are smaller (125 MB block ≈ 190 MB
of model versus 544 MB for the whole corpus), so RAM is not the obstacle.

The obstacle is ratio. `zentropy pblocks <in> --blocks N --jobs M` measures it
exactly — each block is encoded as a standalone archive, every block is decoded,
the pieces are reassembled and compared to the input:

| corpus | blocks | speedup | ratio cost |
|---|---|---|---|
| enwik7 (10 MB) | 8 × 1.25 MB | 9.12× | **+16.28%** (+385,584 B) |
| enwik8 (100 MB) | 8 × 12.5 MB | 6.04× | **+11.96%** (+2,539,567 B) |
| enwik8 (100 MB) | 4 × 25 MB | 3.40× | **+6.19%** (+1,313,372 B) |

A context-mixing model's strength is the cross-block context it accumulates, so
cutting the stream costs it more than the added cores return. For scale: Phase 9's
entire adopted result is **−158,058 B**; a 4-way blocked format loses **+1.31 MB**
on enwik8 — an order of magnitude in the wrong direction. The trade is not close,
and the loss does not shrink enough with block size to become interesting.

**Decision.** Blocking stays a research probe with a printed, exact cost. It is
not in the scored configuration, and it is not a defensible screening proxy
either: it measures a *different representation*, and this project has already
been burned by trusting a smaller rung.

## 4. Where the throughput actually comes from

Ranked by measured effect on a gate's wall-clock:

1. **Not re-paying for the parent.** `eval --parent-archive-bytes <n>` reuses the
   accepted baseline's receipted archive size instead of re-encoding and
   re-decoding it: 4 passes → 2, halving every gate. The candidate is still
   encoded and exactly decoded in the run.
2. **Across-candidate concurrency** (`--jobs`, `tools/gate_many.sh`): N gates in
   ~N/cores the wall-clock.
3. **Screening on enwik8 and gating only winners**: 254 s per encode versus
   ~1,928 s on enwik9.
4. **Cache/layout work on the tables** — the predictor is latency-bound on hashed
   accesses, so this is the only remaining *single-pass* lever.

Item 1 is the largest single win and it was pure waste before: two of `eval`'s four
passes re-derived a number that was already receipted.
