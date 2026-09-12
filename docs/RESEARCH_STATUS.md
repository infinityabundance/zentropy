# Research Status and Roadmap

> Honest status as of this revision. A mechanism is `MEASURED` only when a
> number exists in `evidence/runs/`; everything else is `PROPOSED`. Nothing here
> claims competitiveness against the 110 MB record.

## What is true right now

- **Exactness holds.** 68 unit/property tests plus 7 scripted courts pass. The
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
  (`hoist + column + word-token-reverse + tune 7`) gives enwik9
  `180,079,678` bytes (`1.4406` bpc), decoded byte-identically. Intermediate
  milestones: pre-column `182,949,204` (1.4636 bpc), then hoist+column
  `181,803,607` (1.4544 bpc). This is milestone G0 (exact 10⁹-byte
  reconstruction) and G1 (beats generic compressors).
- **Mechanisms are adopted only by complete, measured cost:** word/bigram
  experts (−653,805 B on enwik8) and orders 0/5/12/16 (−118,676 B on enwik8).
  Structural hoisting is adopted for ≥ enwik7 (−15,130 B on enwik8) but
  **rejected on enwik6 (+322 B)** once its 1,728-byte measured executable cost
  is charged — a direct demonstration that estimates were unsafe.
- **Hutter score accounting is sealed:** the three legal submission forms are
  unit-tested constructors, and no mechanism's adoption decision uses an
  estimated byte cost.
- **The submission path works.** The scored stub (346,208 B) is both `comp9a`
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
  charged 21,848 B of executable. A26's v2 extension of that vocabulary past 255
  tokens is **REJECTED** (coverage loses at every cap tested). See
  [`OPTIMIZATION_PHASE_A.md`](OPTIMIZATION_PHASE_A.md).

## What is *not* true yet

- The floor is roughly **5× larger than the record**. Phases 3–8 are the climb.
- The context-mixing stack uses only *direct* probability models; it has no
  ICM/ISSE bit histories, no state maps, no SSE beyond two APM stages, and no
  bidirectional/structural contexts.
- Structural hoisting exists but is narrow (31 fixed strings). There is still
  no general lexical/phrase dictionary transform, no grammar, no rank/enumerative
  coding, no article reordering, no learned residual model, and no representation
  optimiser.
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
2. **Cheap article-order oracle (Phase 7 probe).** Determine whether the reorder
   signal is large and whether the permutation can be coded cheaper than the
   gain. Do **not** spend serious optimisation effort yet. Established negative
   input: enwik9 pages are not title-sorted, so restoration costs a stored
   permutation.
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
8. **Residual-conditioned transformer last,** when it is genuinely learning the
   difficult remainder rather than compensating for missing classical machinery.

## The milestone that matters

The first decisive question is not whether Zentropy reaches 100 MB. It is:

1. Can a mature classical floor push this to roughly **125–140 MB**? Then
2. Does grammar/rank/proceduralization produce **multiple megabytes of additional
   saving after that floor**, rather than merely rediscovering what the predictor
   already captured?

Today the floor is 180.1 MB and the distinctly-Zentropy machinery has not yet
been deployed. The gap to the pending frontier (`fx2-cmix-transformer`,
100.42 MB including compressor) is ~79.7 MB. It is not close, and the project
does not pretend otherwise.

## Superseded earlier ordering

The previous revision ranked the article-layout compiler first. That is now
item 2/7: a cheap probe, not an optimisation campaign.

## Known risks

- **Runtime.** A full enwik9 pass is tens of minutes here; the judged path must
  fit `70,000/T` hours *on a single core of the reference machine*. Every
  mechanism is gated on that, not on our hardware.
- **Memory.** The model already uses ~450 MB for enwik8; enwik9 needs a
  careful allocation budget under 10 GB.
- **Binary size.** The stub is 346,208 B, of which 21,848 B is the adopted
  word-token transform (mostly `std::HashMap`). If the model grows, the scored
  `compressor_bytes` grows with it; Phase 11 must reclaim this.
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
