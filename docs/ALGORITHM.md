# The Zentropy algorithm

> A standalone description of the submitted method, for a reviewer who will not
> read the source. Every number here is measured; the receipts are named where
> they matter, and the full accounting is in
> [`RESOURCE_CLOSURE.md`](RESOURCE_CLOSURE.md).
>
> **No claim of competitiveness is made.** At this revision the complete score is
> ≈ 165.6 MB against a 109,685,196 B gate derived from the accepted record, i.e.
> ≈ 55.9 MB short. What follows describes what the program *does*, not what it
> beats.

## 1. What is submitted

One program serves both roles (`comp9a == decomp9`), built from this repository:

```sh
cargo +nightly-2026-07-24 -Z build-std=std,panic_abort \
    build --profile submission --no-default-features \
    --features accepted,submission --bin zentropy-sfx \
    --target x86_64-unknown-linux-gnu
RUSTFLAGS="-Zunstable-options -Cpanic=immediate-abort"
```

Two legal forms are produced:

| form | `S` |
|---|---|
| self-extracting | `len(program) + len(program ‖ marker ‖ len ‖ archive)` |
| separate, same program twice | `2 × len(program) + len(archive)` |

The program is **105,536 B**; see §5 for how it is measured. It links glibc and
nothing else — no third-party crate reaches the scored path
([`LICENCE_INVENTORY.md`](LICENCE_INVENTORY.md)).

## 2. The model of the problem

Zentropy treats compression as **finding the shortest executable explanation** of
the corpus, and treats `S = program + archive` as the only authority. Every
mechanism was admitted only when the *complete*, **measured** change in `S` was
negative; none was admitted on bits-per-byte, ratio, or entropy.

Two consequences shape the whole design:

- **The program is charged twice**, so a byte of code is worth two bytes of
  score. Most of Phase 12 is about removing code that cannot execute.
- **Anything the decoder needs must be inside the submitted bytes.** Where a
  corpus-derived structure is used, it is either reconstructed from information
  already in the archive or stored and charged.

## 3. The pipeline

Encoding is a chain of exactly invertible transforms followed by a context-mixing
binary predictor and a range coder. For the scored corpus the chain is:

```
raw enwik9
  ↓  article-layout compiler     (encoder-side reordering; restored for free)
  ↓  structural hoisting         (31-tag dictionary → one-byte codes)
  ↓  word tokenizer              (corpus-derived vocabulary, stored in the archive)
  ↓  context-mixing predictor    (18 experts → mixer → 3 APM stages → residual corrector)
  ↓  binary range coder
archive9.bhm
```

Decoding inverts each step in reverse order. Three properties are structural, not
conventional:

- **Every transform has a universal literal fallback.** A byte the transform does
  not recognise passes through unchanged, so the transforms are exact on
  arbitrary and malformed input, not merely on well-formed XML.
- **The archive is self-describing.** Method, tuning byte, transformed length and
  (where used) a permutation are in the header. The decoder derives the entire
  model geometry from the archive alone.
- **No clock, no randomness, no environment input.** Every decision is a pure
  function of the bytes seen so far.

### 3.1 Article-layout compiler (Phase 7)

Wikipedia's `<page>` blocks are not ordered so that similar pages are adjacent,
but each block **carries its own page id**, and the ids are strictly ascending in
the original file. The encoder therefore reorders pages by (category set,
template set, title) to put semantically similar articles next to each other,
which helps every context model.

The decoder restores the original order by a **stable sort on the embedded page
id** — so the permutation itself costs **zero bytes**. That is only legitimate
because the ids are already in the corpus and ascending; the encoder first checks
that precondition, and if it fails it falls back to the accepted configuration
without the reorder and records the fallback in the header. The evidence that the
ordering is a transform and not hidden knowledge: the identity control is exactly
0 bytes and the shuffle control is +25,519.

### 3.2 Structural hoisting (Phase 3)

A fixed 31-entry table of XML tag prefixes lives in the program's read-only data
and is addressed by bytes `0x01..=0x1F` (`0x00` escapes a literal control byte,
`0x20` is a literal space). This costs **0 archive bytes** and shrinks the
transformed stream by ~18% on enwik9. The census that justifies stopping here: all
XML tags are 2.6% of the corpus, the table covers 87% of them by prefix, and the
largest uncovered tag is worth ~309 archive bytes — inside the cost of editing
the table.

### 3.3 Word tokenizer (A1.1/A26)

A vocabulary is **derived from the corpus itself**, frequency-ordered, and stored
as a prefix of the same modelled stream — so the dictionary is entropy-coded by
the same predictor as the body and its cost is fully accounted, not hand-waved.

A word qualifies when it is at least 3 bytes and occurs at least twice, and only
if a two-byte token recovers more than the entry's definition cost. Tokens are
`0x00 id` (`id` in 1..=255), a literal `0x00` is escaped as `0x00 0x00`, and
everything else is copied verbatim, so the mapping is injective. Adopted at
enwik9 for **−1,723,929 B** against a measured 21,848 B of executable.

Six alternative lexical representations — recency-ranked ids, model-priced
vocabulary, priced filtering, affix families, first-use definitions, and
subword/BPE composition — were each **rejected by measurement** in Phase 10, three
of them despite a screening signal that grew with corpus size. This predictor
does not want a different lexical representation.

### 3.4 The predictor

A binary, bit-by-bit context-mixing model. Each byte is coded as 8 binary
decisions, most-significant bit first.

**Experts (18 inputs to the mixer).** Ten direct bit contexts over the previous
1–16 bytes (orders 0, 1, 2, 3, 4, 5, 6, 8, 12, 16), a word-context expert, a
word-bigram expert, a previous-line/column expert for tabular and markup regions,
a matched-literal expert, a long-distance match tier (minimum match 8), a sparse
match tier (minimum 4 with gap 1), and the APM/SSE-derived inputs. Each direct
expert is a hashed table of 12-bit probabilities, adaptively updated with
`p += (target − p) >> rate`.

**Table geometry (T2).** Each direct expert's table is scaled by
`2^(tune >> 4)` relative to a corpus-size base. The scored point is scale 3, i.e.
2^27 slots per expert at enwik9 (a 2.67 GB model). This single knob is worth
**−3,938,320 B** at enwik9 for a measured 192 B of executable, and it is
decoder-derivable because the scale travels in the archive's own header byte. It
is clamped ([`MAX_TABLE_SCALE`](../src/context/mod.rs)) so that no archive can ask
the decoder for a larger model than the accepted one.

**Adaptation ladder.** Each expert's shift is its memory. The shipped ladder is
`2,2,1,2,2,2,2,3,3,3,2,3,4,5`, measured by a coordinate pass at the adopted
geometry (worth −106,709 B on enwik7 and gated on enwik9). An earlier ladder had
no recorded measurement behind it.

**Mixer.** A logistic mixer with integer weight update
`w += input × (target − prediction) × lr`, at learning rate 16 (an interior
optimum, bracketed on both sides at the adopted geometry: LR 10 is +417,524 and
LR 20 is +189,102 at enwik9).

**Calibration (APM/SSE).** Three adaptive probability-map stages refine the
mixed probability, keyed on the previous byte, a longer context, and an order-2
context respectively. The Phase-9 APM adaptation-shift axis was **rejected**
(+90,996 B at enwik9) and is compiled out; its receipts remain.

**Learned residual corrector (Phase 8).** A **120-byte** quantized one-hidden-layer
network consumes 12 classical features (the mixer and APM outputs, bit position,
match state, word/rep flags) and emits a logit correction. Inference is
**integer-only** — see §6 — and the weights are embedded and charged. Adopted at
enwik9 for **−421,646 B** against 7,848 B of executable; the permuted-weight
control is +2,201,020, which is what shows the correction is signal rather than
capacity.

### 3.5 Entropy coding

A binary range coder with 12-bit probability precision
([`src/entropy`](../src/entropy/mod.rs)). An rANS alternative exists in-tree; the
range coder is what the archive is coded with.

## 4. Why the code is small

Because `S` charges the program twice, Phase 12 removed code that could never
execute. Each removal was verified by proving the **archive byte-identical**,
since a size change that alters the archive is a compression change in disguise.

| removal | per-copy saving |
|---|---|
| the offline trainer, which brought a `log2f` libm dependency into the stub | −2,960 B |
| the 95-arm research model constructor (the scored build can be handed only 2 configurations) | −3,896 B |
| rebuilding `std` with `panic = "immediate-abort"` (Phase 11) | −288,928 B |

The result is 105,536 B, and the property that matters is checkable: the stub's
symbol table contains **no libm call at all**, so the judged binary's output
cannot depend on a floating-point library implementation.

## 5. How the score is measured

```sh
tools/package_sfx.sh evidence/corpus/enwik9
```

builds the pinned toolchain, compresses with that stub, decompresses with that
stub, refuses to proceed unless the reconstruction is byte-identical to the
corpus, then packs the self-extracting form and prints `S` for both legal forms.
The two forms differ by exactly 23 bytes (the SFX marker and the length field),
which is the cross-check that the two accounting paths agree.

## 6. Determinism and portability

**Determinism.** No floating-point operation is reachable in the shipped binary
(§4). No clock, randomness, thread scheduling or environment variable affects any
coded decision. Coding is a pure function of the input and the model state, which
is also why a long research run may be *paused* without changing its output.

**Portability.** The dynamic build needs glibc ≥ 2.34 and is x86-64; a statically
linked musl build and the source-zip form are recorded as remedies in
[`LICENCE_INVENTORY.md`](LICENCE_INVENTORY.md) §4. SIMD is never used in the
scored path (AVX2 was measured *slower*: 1.19–1.51× on the dominant loop).

## 7. Resource use

| resource | measured | limit |
|---|---|---|
| peak RAM, enwik9 encode | 5.63 GiB | 10 GB |
| peak RAM, enwik9 decode | ≈ 5.6 GiB | 10 GB |
| temporary disk | the output file only | 100 GB |
| runtime | ≈ 58 min encode, ≈ 49 min decode, single-threaded | `< 70,000/T` h |

The program is **single-threaded**, so the all-core reading of `T` costs it
nothing.

## 8. What was rejected, and why that matters

The repository records roughly forty measured rejections, each with a negative
control. Three are worth naming because they are the ones most readers would
expect to help:

- **AVX2 intrinsics** in the predictor: 1.19–1.51× *slower* on the dominant loop.
- **Parallel blocking**: 3.4–9.1× faster for **+6.2% to +16.3%** ratio.
- **ICM/ISSE, bit-history state maps, PPM-C**: all win on enwik7/enwik8 and
  **reverse sign at enwik9** (+754,671 and +61,133 respectively). This is why
  enwik9 is the only authority and why nothing is extrapolated upward from a
  smaller rung.

The rejections are kept in-tree as receipts because they are the project's most
transferable knowledge: this predictor already captures much of what those
mechanisms were designed to supply.
