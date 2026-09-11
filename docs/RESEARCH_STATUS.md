# Research Status and Roadmap

> Honest status as of this revision. A mechanism is `MEASURED` only when a
> number exists in `evidence/runs/`; everything else is `PROPOSED`. Nothing here
> claims competitiveness against the 110 MB record.

## What is true right now

- **Exactness holds.** 45 unit/property tests plus 7 scripted courts pass. The
  Wikipedia IR (`ZIR-0`) round-trips arbitrary and malformed input exactly. The
  archive decoder rejects corruption without panicking or allocating without
  bound. A deterministic incompressible stream does not compress.
- **The corpus is pinned.** `enwik8`/`enwik9` digests are recorded in
  `evidence/baseline/CORPUS.sha256` and in code.
- **The floor is real and measured.** `RawCm` reconstructs enwik8 exactly at
  `22,465,931` bytes (1.7973 bpc), beating `xz -9e` (24,831,656), `brotli -q 11`
  (25,742,001), `bzip2 -9` (29,008,758) and `gzip -9` (36,445,248) on the same
  input.
- **The full corpus reconstructs exactly.** enwik9 → `182,949,204` bytes
  (1.4636 bpc) in 41 min, 3.58 GB peak RAM, both directions. This is milestone
  G0 (exact 10⁹-byte reconstruction) and G1 (beats generic compressors).
- **Two mechanisms are adopted by measurement:** word/bigram experts
  (−653,805 B on enwik8) and structural hoisting (−16,315 B on enwik8).
- **The submission path works.** The scored stub (313,792 B) is both `comp9a`
  and `decomp9`; a packed self-extracting `archive9` reconstructs byte-identically
  with no external inputs.

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

## Next highest-value experiments, in order

Ranked by expected (score gain) ÷ (engineering cost × uncertainty × resource
risk), per the transfer analysis in `docs/STACK_TECH_TRANSFER.md` and
`docs/PRIOR_ART_MECHANISMS.md`.

1. **Article-layout compiler (Phase 7).** enwik9's pages are *not* title-sorted
   (verified), so restoration costs a stored permutation, which must be entropy
   coded and charged. But reordering is independently record-winning in both
   `fx2-cmix` and `starlit`. First experiment: a cheap semantic order (page
   title + bag-of-words sketch) with a delta-coded permutation; measure
   `ΔS = Δarchive + permutation_cost` on enwik7/8. Negative results are
   first-class.
2. **Structural hoisting (Phase 3).** Use `ZIR-0` to hoist reconstructible
   markup (fixed tag names, template delimiters, link delimiters) out of the
   modelled stream. Hypothesis: fewer, more regular bytes and shorter match
   distances. Measure the complete cost including restoration logic.
3. **Transformed lexical dictionary + repeat-offset state (Phase 4).** Replace
   frequent surface forms with short codes from a root/transform representation;
   charge the dictionary. Generalise LZMA's repeat-distance state to repeated
   phrase/template/reference state.
4. **ICM/ISSE bit histories and a wider mixer (Phase 6).** Replace direct
   probability models with nibble-aligned bit-history tables and add SSE stages.
   This is the well-trodden path from `lpaq1` to `cmix` and is where most of the
   remaining classical gain lives.
5. **Grammar + rank (Phase 5).** Induce shared procedural descriptions of
   Wikipedia constructs; entropy-code the grammar skeleton; rank-code per-instance
   state. EntropyFS measured a −47% reduction in grammar-skeleton cost, leaving
   the gap explicitly in contextual modelling and rank-coded state.
6. **Residual-conditioned learned corrector (Phase 8).** Train a small
   transformer on the *residual* of the classical predictor, quantise it, charge
   every model byte, and admit only if `net_gain > 0`. This mirrors
   `fx2-cmix-transformer`'s winning structure but on a thinner remainder.
7. **Representation optimiser (Phase 10).** Equivalence-preserving rewrites to a
   fixed point, accepted only when `Decode(D1) == Decode(D0)` and
   `Size(D1) < Size(D0)`.

## Known risks

- **Runtime.** A full enwik9 pass is tens of minutes here; the judged path must
  fit `70,000/T` hours *on a single core of the reference machine*. Every
  mechanism is gated on that, not on our hardware.
- **Memory.** The model already uses ~450 MB for enwik8; enwik9 needs a
  careful allocation budget under 10 GB.
- **Binary size.** The stub is 313,792 B. If the model grows, the scored
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
