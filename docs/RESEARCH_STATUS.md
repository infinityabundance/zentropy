# Research Status and Roadmap

> Honest status as of this revision. A mechanism is `MEASURED` only when a
> number exists in `evidence/runs/`; everything else is `PROPOSED`. Nothing here
> claims competitiveness against the 110 MB record.

## What is true right now

- **Exactness holds.** 119 unit/property tests plus 8 scripted courts pass. The
  Wikipedia IR (`ZIR-0`) round-trips arbitrary and malformed input exactly. The
  archive decoder rejects corruption without panicking or allocating without
  bound. A deterministic incompressible stream does not compress.
- **The corpus is pinned.** `enwik8`/`enwik9` digests are recorded in
  `evidence/baseline/CORPUS.sha256` and in code.
- **The floor is real and measured.** `RawCm` reconstructs enwik8 exactly at
  `22,449,073` bytes (1.7959 bpc) with structural hoisting enabled, beating
  `xz -9e` (24,831,656), `brotli -q 11` (25,742,001), `bzip2 -9` (29,008,758)
  and `gzip -9` (36,445,248) on the same input.
- **The full corpus reconstructs exactly.** The accepted configuration
  (`Method::Residual` at **`tune 5`** — mixer LR 16) gives enwik9
  `169,282,339` bytes (`1.3543` bpc), decoded byte-identically. Progression:
  pre-column `182,949,204` (1.4636 bpc); hoist+column `181,803,607` (1.4544);
  +A1.1 tokenizer `180,079,678` (1.4406); +Phase 4 `176,204,762` (1.4096);
  +Phase 6 SSE `174,533,527` (1.3963); +Phase 7 article layout `170,063,733`
  (1.3605); +Phase 8 learned residual `169,642,087` (1.3571); **+Phase 9 LR
  re-tune `169,282,339` (1.3543)**. This is milestone G0 (exact 10⁹-byte
  reconstruction) and G1.
- **Mechanisms are adopted only by complete, measured cost:** word/bigram
  experts (−653,805 B on enwik8) and orders 0/5/12/16 (−118,676 B on enwik8).
  Structural hoisting is adopted for ≥ enwik7 (−15,130 B on enwik8) but
  **rejected on enwik6 (+322 B)** once its 1,728-byte measured executable cost
  is charged — a direct demonstration that estimates were unsafe.
- **Hutter score accounting is sealed:** the three legal submission forms are
  unit-tested constructors, and no mechanism's adoption decision uses an
  estimated byte cost.
- **The submission path works.** The scored stub (399,616 B,
`--profile submission --no-default-features --features accepted`) is both `comp9a`
and `decomp9`; a packed self-extracting `archive9` reconstructs byte-identically
with no external inputs.
- **Optimization Phase A is running.** A17 (previous-line/column expert) and the
  A20 learning-rate variant (`tune 7`) are adopted; A2 (alphabet permutation) and
  A3 (information inheritance) are rejected, each with a negative control that
  demonstrates the mechanism is real but the heuristic is wrong. A1.2 case
  factorization is **rejected in full**: merging lexical identity wins on
  enwik6/7 and reverses on enwik8, and marking alone wins on enwik7/enwik8 then
  reverses by +427,246 B on enwik9. A1.1/A26 dynamic word tokenization is
  **ADOPTED** (`column-word-token-reverse`): −1,723,929 B archive at enwik9 for a
  charged 21,848 B of executable. **Phase 4 is complete**: the long-distance and
  sparse match tiers and the matched-literal expert are adopted (enwik9
  −3,869,340 fully accounted), while repeat-offset state, distance-conditioned
  floors, stemming, word-class, phrase/affix dictionaries and front-coding were
  rejected with controls and dose-response. See
  [`OPTIMIZATION_PHASE_A.md`](OPTIMIZATION_PHASE_A.md) and
  [`PHASE4_PLAN.md`](PHASE4_PLAN.md).

- **Phase 6 (context-mixing spine) is complete.** The extra order-2 SSE stage
  (`sse-3`) is **ADOPTED**: enwik9 **−1,671,235 B** (176,204,762 → 174,533,527,
  1.3963 bpc) for a measured 256 B of executable. Its control, the same stage
  keyed on an uncorrelated distant byte (`sse-3-ctl`), is only −305,341: the
  information in the order-2 key is worth ≈1.37 MB. Bit-history state maps,
  ICM/ISSE, sparse contexts, collision control, the stem model, the bounded
  PPM-C expert and high-order pruning are **REJECTED** with controls. The phase's
  central lesson is that enwik9 and the mid-scale rungs can disagree in *both*
  directions: state maps win at enwik7/8 and reverse to **+754,671** at enwik9,
  and PPM-C wins at enwik7/8 (−49,505 / −121,466) and reverses to **+61,133**.
  See [`PHASE6_PLAN.md`](PHASE6_PLAN.md).

- **Phase 7 (article-layout compiler) is complete and is the largest single
  mechanism so far.** enwik9 pages are not title-sorted, but their page ids are
  strictly ascending and travel inside each block, so the original order is
  restored by a **free** stable sort on the id — the permutation costs 0 bytes
  where an explicit one would cost 500,557. The encoder orders pages by their
  category set, then template set, then title. Adopted at enwik9
  **−4,469,794 B** (174,533,527 → 170,063,733) for a measured 24,208 B. The
  identity control is exactly 0 and the shuffle control is +25,519, so the gain
  is the ordering and not the machinery. Content-similarity orders (word
  MinHash, greedy nearest-neighbour) *lose*; shared markup, not semantic
  proximity, is what the predictor exploits. See [`PHASE7_PLAN.md`](PHASE7_PLAN.md).

- **Phase 8 (learned residual corrector) is complete.** A 120-byte quantized
  integer MLP consumes the classical mixer/APM outputs and emits a logit
  correction, trained offline and embedded in the binary. Adopted at enwik9
  **−421,646 B** (170,063,733 → 169,642,087, 1.3571 bpc) for a measured 7,848 B;
  the permuted-weight control is +2,201,020. The phase's net-gain gate
  (`residual_saved > model_bytes + binary_bytes`) passes: 421,646 > 7,848. A unit
  test comparing the integer runtime against a dequantized float replication of
  the trainer caught a real bias-scaling bug that had made the shipped network
  5–17 MB *worse* than the parent. See [`PHASE8_PLAN.md`](PHASE8_PLAN.md).

- **Phase 9 (global search) is complete, and its headline result is that the
  mixer learning rate is a function of the predictor and of scale.** The `tune`
  byte became a two-axis hyperparameter vector (mixer LR in the low nibble, APM
  adaptation shifts in the high nibble) behind a receipted search layer —
  frf-fuzz mutation, a DSFB observer, Pareto reporting, and a Gemel memory scoped
  by corpus digest that never re-pays for a known configuration. Campaigns were
  exhaustive on enwik7 (256 points), coordinate on enwik8 (46), and gated by full
  `eval` on enwik9. **ADOPTED: mixer LR 24 → 16, enwik9 −359,748 B at zero
  executable cost** (LR 20 was −158,058; every rung down bought more, so the next
  rung was gated rather than extrapolated). **REJECTED at scale: the APM
  adaptation-shift axis** — worth ≈12 KB on the mean at enwik7 and within 653 B at
  enwik8, it is **+90,996 B at the best LR on enwik9**, and is compiled out. The
  phase nets −224 B of executable. Three throughput proposals were measured and
  rejected rather than argued about: AVX2 is **1.19–1.51× slower** on the dominant
  random-table loop ([`SIMD_DECISION.md`](SIMD_DECISION.md)); parallel blocking
  costs **+6.2% to +16.3%** ratio for 3.4–9.1× speed
  ([`PARALLELISM_DECISION.md`](PARALLELISM_DECISION.md)); rayon threads are adopted
  for the research plane only (3.0× on four concurrent tunes, byte-identical,
  ~16 B if ever linked). See [`PHASE9_PLAN.md`](PHASE9_PLAN.md).

## What is *not* true yet

- The floor is roughly **5× larger than the record**. Phases 3–8 are the climb.
- The context-mixing stack now has a real calibration stage, a PPM expert and a
  learned residual corrector, but still no ICM/ISSE bit histories and no
  structural/bidirectional contexts. The corrector is tiny (120 B / 4 hidden
  units); larger learned models remain a Pareto option, not a claim.
- **Phase 3 (structural modeling on the IR) is closed by measurement.** The
  31-entry structural hoist is adopted and saturated: covering the largest
  remaining tag gap (34.5 KB raw) changes the archive by only −309 B, and all
  metadata field hoisting is bounded at ≤1.3% of the archive while naive binary
  packing loses. The IR (ZIR-0) is exact but not worth wiring into the pipeline.
- **Phase 5 (procedural grammar + rank/enumerative state) is closed by
  measurement.** Every mechanism — iterative RePair, maximal-repeat induction,
  one-shot scalable induction, first-use inline productions, rank-ordered rule
  tables, MTF symbol ranking and LZBE factorization — loses against the accepted
  floor, and the loss *grows* with corpus size (`grammar-oneshot` +67,989 enwik6 →
  +489,509 enwik7; `lzbe` +150,530 → +1,784,136). The CM's Phase-4 match family
  already owns the repetition a grammar would explain, and writing the
  explanation down explicitly is a net cost. Rank coding of symbols (MTF) is
  actively harmful (+198,511 vs +47,298 verbatim at enwik6). This is a decisive
  negative result for the project's central hypothesis on enwik. See
  [`PHASE5_PLAN.md`](PHASE5_PLAN.md).
- The submission stub has not been size-optimised (Phase 11).
- The model has only been timed on this machine, not on a Geigerbench-scored
  reference machine.

## Accounting status (fixed this revision)

Two score-accounting leaks were found and closed. Both were bookkeeping, not
compression, but both could have inflated a later celebration:

1. **Separate-form score undercounted by one program copy.** The rule is
   `comp9a + 2×decomp9 + bhm`, reduced to `comp9a + decomp9 + bhm = 2P + bhm`
   when `comp9a == decomp9`. `tools/package_sfx.sh` had printed `P + bhm`. The
   three legal forms now live as named, unit-tested constructors in
   [`score`](../src/score/mod.rs): `self_extracting`, `separate`,
   `shared_program`.
2. **Phase-3 executable cost was estimated, not measured.** The old
   `binary_cost_estimate()` (543 B: dictionary bytes + an arbitrary allowance)
   was deleted. `tools/measure_binary_cost.sh` now builds an otherwise-identical
   submission binary with and without `--features struct-hoist` and reports the
   real delta. Measured cost: **1,728 B**. With correct accounting the structural
   hoist is **REJECTED on enwik6 (+322 B)** and adopted on enwik7 (−4,472 B) and
   enwik8 (−15,130 B) — it only pays once there is enough markup to amortise the
   fixed executable cost.

## Next highest-value experiments, in order

The ordering below was revised after external review. Its guiding insight: the
optimal article order is a function of the predictor, so a layout optimised
against today's simple direct-context predictor could become a local optimum
once the classical machinery matures. Probe the signal cheaply now; optimise it
only after the predictor is strong.

1. **Seal exact Hutter accounting.** *(done this revision — see above.)* No
   further research until every reported `S` charges the correct program copies
   and every mechanism's binary cost is measured.
2. **Cheap article-order oracle (Phase 7 probe).** *(done — Phase 7 complete;
   the layout compiler is adopted at enwik9 −4,469,794 B. See PHASE7_PLAN.md.)*
3. **ICM + state maps + ISSE + proper SSE / context-dependent mixing (Phase 6).**
   The largest obvious block of technology still missing. This is the well-worn
   path from `lpaq1` toward `cmix` and is where most remaining classical gain
   lives.
4. **Better match modelling.** Sparse matches, multiple recent matches, match
   confidence/state, and word/structural matches.
5. **Structural factorization and transformed lexical/phrase representation
   (Phases 3–4).** Generalise the 31-entry hoist into a real dictionary with
   root/transform representation and repeat-offset state.
6. **The distinctly Zentropy part: grammar / configuration / rank / procedural
   seed explanations (Phase 5).** This is the first experiment that actually
   tests the project's central hypothesis rather than rediscovering known
   compression technology.
7. **Re-run article-layout optimisation against the now-strong predictor.**
   *(done — Phase 7; the layout is now part of the accepted configuration.)*
8. **Residual-conditioned transformer last,** when the learned corrector's
   residual saturates. *(Phase 8 built and adopted a 120-byte MLP corrector at
   enwik9 −421,646 B; the transformer remains a width/architecture point on the
   same Pareto curve and is only worth revisiting if the MLP saturates.)*

## The milestone that matters

The first decisive question is not whether Zentropy reaches 100 MB. It is:

1. Can a mature classical floor push this to roughly **125–140 MB**? Then
2. Does grammar/rank/proceduralization produce **multiple megabytes of additional
   saving after that floor**, rather than merely rediscovering what the predictor
   already captured?

Today the floor is 169.3 MB (archive) / ~169.7 MB complete `S`. The gap to the pending
frontier (`fx2-cmix-transformer`, 100.42 MB including compressor) is ~68.9 MB; to
the accepted record (`fx2-cmix`, 110.79 MB) it is ~58.5 MB. It is not close, and
the project does not pretend otherwise.

## Superseded earlier ordering

The previous revision ranked the article-layout compiler first. That is now
item 2/7: a cheap probe, not an optimisation campaign.

## Known risks

- **Runtime.** A full enwik9 pass is tens of minutes here; the judged path must
  fit `70,000/T` hours *on a single core of the reference machine*. Every
  mechanism is gated on that, not on our hardware.
- **Memory.** The model already uses ~450 MB for enwik8; enwik9 needs a
  careful allocation budget under 10 GB.
- **Binary size.** The stub is 399,616 B. Phases 6–9 added ≈33 KB of dispatch and
  mechanism code; Phase 11 must gate the rejected methods out of the submission
  build. The accepted mechanisms' own marginal costs are small (SSE 256 B,
  reorder 24,208 B, learned 7,848 B).
- **Determinism.** All arithmetic is integer-only today. This must be preserved
  if any floating-point learned component enters the submission.

## How to reproduce

```sh
cargo test
cargo build --release
./target/release/zentropy bench evidence/corpus/enwik8 --receipt evidence/runs/receipts.jsonl
sh tools/baseline.sh  evidence/corpus/enwik8 evidence/baseline/enwik8.jsonl
sh tools/courts.sh
sh tools/package_sfx.sh evidence/corpus/enwik6
```
