# Zentropy — Prior-Art Mechanisms (reimplementable, measurable)

**Scope.** This document isolates the strongest *mechanisms* — not brands, products, or
benchmarks — from twelve lines of prior art relevant to enwik9 / Hutter-Prize-style lossless
compression, and states for each what to build, where it belongs in Zentropy's research vs
submission plane, what it is estimated to cost in scored bytes and runtime, the information
gain to expect, the experiment that decides it, and its transfer status. The operative
metric throughout is the Hutter score `S = len(submitted_compressor) + len(self_extracting_archive)`,
subject to exact reconstruction, no external data, no GPU in the judged run, ≤10 GB RAM and
≤100 GB temporary disk, and an OSI-licensed source. Every mechanism is expressed so that a
single implementer can (a) write the code, (b) run one ablation on a bounded enwik slice, and
(c) record a ΔS. Rows whose mechanism is known only from general domain knowledge are marked
`UNVERIFIED (domain knowledge)`; rows grounded in a page actually fetched for this document
are marked `VERIFIED (source)` and listed in "Verified sources".

**Retrieved date / source note.** Retrieved 2026-09-11. Three pages were fetched successfully:
the `fx2-cmix` README, the `starlit` README, and Bellard's `nncp` page (see "Verified
sources"). Three fetches 404'd (`fx-deepmix`, `cmix-lex`, `zpaq` READMEs) and were abandoned
per the context budget; the corresponding rows are marked as domain knowledge. The
headline numbers attributed to the *pending frontier* (`fx2-cmix-transformer` archive9 =
96,996,198; 96,994,188 on Intel with a segfault fix; total ≈100,424,672; +2.9 MB executable;
6 M-parameter transformer pretrained on enwik9 on 8×RTX5090 for 26 h; CPU-only decode),
`tufazip` (105,924,360 + 54,709), `altxs` (93,434,410 + 13,490,401), `nncp v3.2`
(106,632,363 + 628,955), `fx-deepmix` (107,828,411), and the official record
`fx2-cmix` (S = 110,793,128, accepted 2024-10-08, L = 110,793,128, 1% gate) are taken from the
in-repo verified baseline (the task brief and `zentropy/prompt.txt`), **not** re-derived here.

---

## How to read this

**Status vocabulary.**

- `PROPOSED` — a transfer hypothesis not yet measured on Zentropy; the default.
- `MEASURED` — a Zentropy-side measurement exists. **Initially none**; source-side numbers are
  cited in prose and are never conflated with this column.
- `ADOPTED` — a zero-scored-byte discipline that is already constitutionally binding (exactness,
  accounting, literal fallback, decode-has-no-search-authority).
- `REJECTED` — an anti-mechanism Zentropy must not accept (e.g. external weights or GPU in the
  judged run).
- `SUPERSEDED` — a later mechanism in the same lineage subsumes it.
- `UNRESOLVED_REFERENCE` — the named system could not be identified; **no mechanism is
  attributed**.
- `UNVERIFIED` — the mechanism is asserted from domain knowledge only.

Because upstream mechanism facts and Zentropy transfer status are different axes, the Status
cell uses a compound form: a transfer status plus a provenance qualifier, e.g.
`PROPOSED / VERIFIED (source)` or `PROPOSED / UNVERIFIED (domain knowledge)`. Where a row is an
outright anti-mechanism it is `REJECTED / UNVERIFIED (domain knowledge)`.

**Column conventions.**

- *Reusable code?* — `yes`, `no`, or `partial`, plus a license caveat. Licensing is only
  asserted where it is well established (Brotli MIT, Zstandard BSD/GPLv2 dual, LZ4 BSD-2,
  XZ/LZMA-SDK public-domain, ZPAQ MIT/public-domain); all others say `license: verify`.
- *Research-plane role* — what it does while searching (encoder-only work is free).
- *Submission-plane role* — what must survive into the decoder and therefore count.
- *Est. byte cost* — scored bytes added to `S` including model, dictionary, weights, and code.
  `0` means the mechanism is encoder-side only or is a discipline with no payload.
- *Est. runtime cost* — judged-path (decode) runtime, not encode feasibility.
- *Expected information gain* — an engineering **estimate (EST)**, to be replaced by a measured
  ΔS; no number here is a claim.
- *Experiment required* — the one ablation that decides the row.
- `[DK]` in a mechanism name is shorthand for domain-knowledge provenance.

**Phases.** The cross-cutting ranking uses Zentropy's existing phase ladder: Phase 0
constitutional laws; Phase 1 source-native reversible IR; Phase 2 entropy coders; Phase 3
structural modeling on the IR; Phase 4 dictionary + match + repeat-offset state; Phase 5
grammar + rank state; Phase 6 model/table reuse and content addressing; Phase 7 article-layout
compiler; Phase 8 residual-conditioned learned corrector; Phase 9 DSFB/search observer;
Phase 10 representation rewriting to fixpoint.

---

## 1. Brotli (RFC 7932)

Brotli's transferable core is a *fixed natural-language side channel* (a shipped dictionary plus
word transforms) glued to a *literal-context model* and a compact prefix-code representation.
It is a decisive prior-art point for enwik9 because Wikipedia text is precisely the corpus its
dictionary targets. No page was fetched for this section: all rows are `[DK]` from RFC 7932.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **B1** Static dictionary of 13,504 words + prefix/suffix/case transforms | A frozen word list makes common words addressable by index + transform instead of spelled literals | yes (MIT) | baseline for a shipped dictionary | dictionary bytes count; index references are ~1–2 bytes | dictionary payload, tens of KB compressed | negligible | medium–high on enwik9 | Measure `ΔS` of dictionary on/off over an enwik9 slice; compare against a corpus-trained dictionary of the same scored size | PROPOSED / UNVERIFIED (domain knowledge) |
| **B2** Literal context ID from previous two bytes via a fixed CMAP table + 2 context modes | Cheap, bounded literal context without per-context modeling | yes (MIT) | feature for a literal coder | table bytes in binary; decode-only table | hundreds of bytes | negligible | low–medium | Ablate CMAP vs a direct hashed order-2 literal context | PROPOSED / UNVERIFIED (domain knowledge) |
| **B3** Context map (map histograms to context IDs) | Cluster similar literal contexts so prefix codes amortize | yes (MIT) | entropy-coder clustering | per-metablock context map | small | low | low–medium | Compare clustered vs unclustered histograms on markup-heavy spans | PROPOSED / UNVERIFIED (domain knowledge) |
| **B4** Compact prefix-code representation (simple/complex codes + code-length code) | Encode the code tables themselves cheaply | yes (MIT) | table serialization | header bytes | tens of bytes per table | low | low | Measure table-header overhead vs FSE/Huffman headers | PROPOSED / UNVERIFIED (domain knowledge) |
| **B5** Ring buffer of recent distances + short distance codes | Recency of match distances is itself predictable | yes (MIT) | match-distance modeling | cache in decoder, no payload | 0 | low | medium (markup repetition) | Add a 16-entry distance cache to the Zentropy match model; measure ΔS | PROPOSED / UNVERIFIED (domain knowledge) |
| **B6** Metablocks with three block categories and block splitting | Heterogeneous text switches statistics; allow re-clustering | yes (MIT) | segmentation of the IR stream | block headers | small | low | low–medium | Compare fixed blocks vs statistic-triggered splits on mixed text/markup | PROPOSED / UNVERIFIED (domain knowledge) |
| **B7** Encoder quality ladder + window bits (10–24) | Effort and memory are explicit levers, decoded from a header | yes (MIT) | parameter search | a few header bits | 0 | decoder window RAM | enables tuning | Sweep window size vs ΔS under the 10 GB cap | PROPOSED / UNVERIFIED (domain knowledge) |
| **B8** Dictionary references as the dominant win for natural-language words | The dictionary is a *word* prior, not a general LZ prior | yes (MIT) | design principle | same as B1 | — | — | explains B1 | Inspect which references fire on enwik; classify word vs markup hits | PROPOSED / UNVERIFIED (domain knowledge) |

**Build / measure.** Implement B1 as a transform over the Phase-1 IR: replace word tokens by
(index, transform) with a `Literal` fallback, then feed references to the Phase-4 match model.
Decode contract: the dictionary is embedded and its bytes counted. Measure ΔS against the same
tokenizer with no dictionary, and against a dictionary learned from the training half only.

---

## 2. Zstandard (RFC 8878)

Zstandard's transferable core is *finite-state entropy coding (FSE/tANS)* applied to a
*decomposed LZ sequence model with explicit repeat offsets and reusable entropy tables*.
No page was fetched: all rows `[DK]` from RFC 8878.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **Z1** FSE/tANS finite-state entropy coding | Table-driven entropy coder with near-arithmetic efficiency and fast decode | yes (BSD/GPLv2 dual) | side-stream coder | coder + tables in binary | code bytes + per-stream table bytes | very low | medium | Compare FSE vs range coder on identical side-stream histograms; pick by emitted bytes | PROPOSED / UNVERIFIED (domain knowledge) |
| **Z2** Sequence model with separate literal-length, match-length, offset codes | Why each sequence element occurs is a separate distribution | yes | sequence representation | decode logic | 0 | low | medium | Split Zentropy sequence symbols and measure ΔS vs joint coding | PROPOSED / UNVERIFIED (domain knowledge) |
| **Z3** Three repeat offsets + repeat codes | Most matches reuse a small recent offset set; encode reuse with 1–2 bits | yes | match model | decoder state; no payload | 0 | low | medium–high on markup/templates | Add 3-offset rep state (B5 generalization); measure ΔS and code savings distribution | PROPOSED / UNVERIFIED (domain knowledge) |
| **Z4** Literal Huffman with 4 parallel streams + repeat-table reuse | Throughput via stream splitting; reuse avoids re-sending tables | yes | literal coder | table bytes | small | low | low | Compare 1 vs 4 streams; confirm no ratio loss and measure table reuse rate | PROPOSED / UNVERIFIED (domain knowledge) |
| **Z5** Entropy-table modes (predefined, RLE, FSE, repeat) | Tables are expensive; choose the cheapest description mode | yes | header encoding | mode bits + table bytes | small | low | low–medium | Implement mode selection by complete cost and log the chosen mode mix | PROPOSED / UNVERIFIED (domain knowledge) |
| **Z6** Long-distance matching (LDM) with a rolling hash | Far repeats need a different index than the short-range hash chain | yes | finder for large windows | none (encoder search) | 0 | encoder RAM/time | medium (duplicate boilerplate) | Enable LDM on the IR with a large window; measure ΔS and search cost | PROPOSED / UNVERIFIED (domain knowledge) |
| **Z7** Dictionary/prefix with its own entropy tables | A shared prefix is a reusable model, not just bytes | yes | reuse mechanism | dict + tables count | dict bytes | low | medium | Compare shipping a raw dict vs dict + trained tables | PROPOSED / UNVERIFIED (domain knowledge) |
| **Z8** Optimal parser (btopt) + binary-tree match finder | Encoder may be arbitrarily expensive; decode stays simple | yes | encoder search | none beyond format | 0 | encoder-only | medium | Swap greedy for optimal parse; measure ΔS to size the parse-search budget | PROPOSED / UNVERIFIED (domain knowledge) |
| **Z9** Strategy ladder (fast → btopt) | One binary, many effort/ratio operating points | yes | research ablation tooling | strategy id bit(s) | ~0 | varies | enables all other rows | Add a strategy flag to Zentropy's research harness for A/B ablations | PROPOSED / UNVERIFIED (domain knowledge) |

**Build / measure.** Adopt Z1+Z3+Z5 as the *side-stream* layer of Phase 2 and the match-rep
state of Phase 4. Measure the fraction of sequences using rep codes, and whether FSE beats the
range coder stream-by-stream.

---

## 3. LZ4

LZ4's transferable core is a *decode-speed budget as a first-class design constraint* and a
token byte that packs both literal and match lengths. Its ratio is far below Zentropy's target,
so most rows are `SUPERSEDED` as a compression mechanism but `PROPOSED` as a *fast-path /
fallback* discipline. No page fetched: all `[DK]`.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **L1** Single token byte (4-bit literal length + 4-bit match length) + extension bytes | Compact sequence header | yes (BSD-2) | fast-path sequence format | decode logic | 0 | very low | low | Compare token format vs FSE sequence format on the same stream | SUPERSEDED / UNVERIFIED (domain knowledge) |
| **L2** 4-byte minimum match, 16-bit offset | Cheap finder, 64 KB window | yes | research fast path | — | 0 | low | negative on enwik | Measure ratio loss vs Zstd | SUPERSEDED / UNVERIFIED (domain knowledge) |
| **L3** Frame format with linked/independent blocks, content checksum, skippable frames | Self-describing, extensible container | yes | container design | frame headers | small | low | low | Adopt skippable-frame idea for side channels/sidecars | PROPOSED / UNVERIFIED (domain knowledge) |
| **L4** HC / optimal parse | Encoder effort buys ratio without decode cost | yes | encoder search | none | 0 | encoder-only | low | Confirm optimal parse beats greedy by a measurable ΔS | PROPOSED / UNVERIFIED (domain knowledge) |
| **L5** Dictionary support | Seed the window with a prefix | yes | warm-start | dict bytes | dict bytes | low | low–medium | Compare warm-start dict vs cold start on per-article deltas | PROPOSED / UNVERIFIED (domain knowledge) |
| **L6** Explicit GB/s decode budget | A measurable non-functional constraint on format choices | yes (discipline) | guards against un-runnable decoders | — | 0 | bounds decode | prevents disqualification | Record decode throughput for every candidate codec | ADOPTED / UNVERIFIED (domain knowledge) |
| **L7** No entropy coding of literals (raw) | Simplicity is a legitimate operating point | yes | negative control | — | 0 | very low | negative (baseline) | Keep as the `A0` floor for ablations | ADOPTED / UNVERIFIED (domain knowledge) |

**Build / measure.** Use L4–L7 as an encoder fast-path and as `A0` floor only. Record decode
throughput alongside ΔS; a mechanism that cannot decode within the wall-clock budget is
`RESEARCH_ONLY`.

---

## 4. XZ / LZMA2

LZMA's transferable core is a *binary adaptive range coder over structured context*, a
*state-conditioned literal coder*, and a *chunked container (LZMA2) that resets state to bound
RAM*. The range coder is the natural fit for prediction-heavy text where Zentropy will emit
sub-bit probabilities. No page fetched: all `[DK]`.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **X1** Binary range coder with adaptive bit models | Prediction-heavy coding with near-zero overhead | yes (public domain, LZMA SDK) | core back end for CM | coder + bit models in binary | code bytes | low | high (enables CM) | Compare range coder vs rANS on the same probability stream | PROPOSED / UNVERIFIED (domain knowledge) |
| **X2** Literal coder with `lc/lp/pb` and matched-literal path | Literal probability depends on prior context and on whether a match was predicted | yes | literal modeling | decode logic | 0 | low | medium | Sweep `lc/lp/pb` on an enwik9 slice | PROPOSED / UNVERIFIED (domain knowledge) |
| **X3** Matched-literal coder | A different coder fires when the match byte differs | yes | model conditioning | decode logic | 0 | low | low–medium | Ablate the matched-literals path | PROPOSED / UNVERIFIED (domain knowledge) |
| **X4** 12-state LZMA state machine | Coder choice conditioned on recent literal/match history | yes | context conditioning | state in decoder | 0 | low | low–medium | Compare state machine vs stateless coder selection | PROPOSED / UNVERIFIED (domain knowledge) |
| **X5** pos-slot distance encoding + align bits | Distance magnitude is coded in slot/offset form | yes | distance modeling | decode logic | 0 | low | low–medium | Ablate slot coding vs direct distance code | PROPOSED / UNVERIFIED (domain knowledge) |
| **X6** Four rep distances + short rep | Same insight as Z3, with an explicit short-rep symbol | yes | match model | decoder state | 0 | low | medium | Compare rep-depth 1/2/4 by complete cost | PROPOSED / UNVERIFIED (domain knowledge) |
| **X7** LZMA2 chunked container with state reset and uncompressed chunks | Bound memory and restart statistics where the source changes | yes | segmentation | chunk headers | small | low | medium (RAM compliance) | Measure peak RAM and ΔS with chunk boundaries at article edges | PROPOSED / UNVERIFIED (domain knowledge) |
| **X8** BCJ / Delta reversible filters | Instructions and numeric fields have preconditioned structure | yes | invertible preprocessors | filter flag; exact round-trip required | 0–small | low | low on enwik (little code) | Verify round-trip and measure ΔS on embedded code/numbers | PROPOSED / UNVERIFIED (domain knowledge) |
| **X9** Optimal parse via price dynamic programming | Literal vs match decisions priced by the real coder | yes | encoder search | none | 0 | encoder-only | medium | Feed real range-coder prices into the Phase-4 parser | PROPOSED / UNVERIFIED (domain knowledge) |

**Build / measure.** Make X1 the Phase-2 default for prediction-heavy streams and X3/X4/X6 the
literal/match conditioning. Deliverable measurement: bits/byte of the range coder vs rANS on
identical streams, plus peak RAM with X7 chunking.

---

## 5. Bzip3 / BWT family (bzip2, bzip3, PPMd-adjacent)

The BWT family's transferable core is *reversible global permutation that exposes high-order
redundancy*, plus *runs/MTF and a context-mixing arithmetic back end*. BWT is deterministic and
cheap to invert, which makes it attractive as an *optional* Phase-10 reordering for
long-range-redundant spans; on enwik9 it ranks below context mixing. No page fetched: all
`[DK]`.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **W1** BWT + primary index | A reversible permutation groups similar suffixes | yes (BSD-like/libdivsufsort; bzip3 license: verify) | optional reordering | primary index + inverse | index bytes | O(n) inverse | low–medium | Apply BWT to a bounded article slice; measure ΔS vs no BWT with a CM back end | PROPOSED / UNVERIFIED (domain knowledge) |
| **W2** Suffix-array construction (SA-IS / divsufsort) | Efficient BWT of large blocks | yes (divsufsort; verify) | block construction | none (encoder-side) | 0 | encoder RAM/time | enables W1 | Bound BWT block size under 100 GB temp disk; measure peak temp usage | PROPOSED / UNVERIFIED (domain knowledge) |
| **W3** MTF + RLE over BWT output | Turns local symbol clustering into runs | yes | pre-entropy transform | decode logic | 0 | low | low–medium | Compare MTF+RLE vs direct CM on BWT output | PROPOSED / UNVERIFIED (domain knowledge) |
| **W4** LZP preprocessing before BWT (bzip3) | Predict runs of repeated context, emit literal on miss | yes | pre-transform | decode logic | 0 | low | low–medium | Ablate LZP before BWT | PROPOSED / UNVERIFIED (domain knowledge) |
| **W5** Multiple Huffman tables + selector (bzip2) | Blocks of BWT output have different statistics | yes | entropy back end | table + selector bytes | small | low | low | Compare multi-table Huffman vs single CM back end | SUPERSEDED / UNVERIFIED (domain knowledge) |
| **W6** Context-mixing arithmetic back end (bzip3) | BWT output is coded by a mixer, not static Huffman | yes (verify) | entropy back end | model bytes | model bytes | medium | medium | Compare bzip3 back end vs Zentropy CM on identical BWT streams | PROPOSED / UNVERIFIED (domain knowledge) |
| **W7** Large block sizes (bzip3 up to hundreds of MB) | Larger BWT blocks expose more redundancy but cost RAM/disk | yes | parameter search | block size in header | 0 | RAM/disk | low–medium | Sweep block size against temp-disk and RAM caps | PROPOSED / UNVERIFIED (domain knowledge) |
| **W8** Cheap inverse BWT | Decode-side linear-time reversal | yes | — | inverse code in binary | code bytes | O(n) | enables W1 | Verify exact round-trip on the slice | PROPOSED / UNVERIFIED (domain knowledge) |
| **W9** BWT as a *ranking* primitive, weaker than CM on enwik | Permutation exposes order-k redundancy; CM models it directly | yes | negative control | — | — | — | explains rank | Ablate BWT+CM vs CM alone; if not positive, drop | SUPERSEDED / UNVERIFIED (domain knowledge) |

**Build / measure.** Treat W1–W4 as an optional, evidence-gated reordering of Phase 10. Deliverable
measurement: ΔS of BWT+back-end vs back-end alone, plus peak temp-disk.

---

## 6. PAQ8 family (paq8, paq8px, paq8pxd)

The PAQ8 family's transferable core is the *bit-level context-mixing architecture*: many
hashed context models → logistic mixer → SSE/APM chain → arithmetic coder, with a match model
and word/sparse/indirect models as the enwik-relevant predictors. This is the single most
directly reusable algorithmic template for Zentropy's classical path. No page fetched: all
`[DK]`.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **P1** Bit-level logistic mixing | Combine heterogeneous predictions in log-odds space conditioned on a selector | yes (GPL; verify) | core predictor | mixer weights in binary/state | weights + code | medium | high | Implement a 2-model mixer, then scale; measure bits/bit saved | PROPOSED / UNVERIFIED (domain knowledge) |
| **P2** APM / SSE chain | Refine a probability using a small context table | yes | post-mixer calibration | table bytes | small | low | medium | Ablate APM stages one at a time | PROPOSED / UNVERIFIED (domain knowledge) |
| **P3** ContextMap / StateMap over bit-history states | Map a hashed context to a compact bit-history then to a probability | yes | core context store | state/table bytes | model bytes | medium | high | Compare direct probability tables vs bit-history state maps | PROPOSED / UNVERIFIED (domain knowledge) |
| **P4** Match model | Predict the next bit from the longest recent match and its byte | yes | long-range predictor | hash table bytes | model bytes | medium | high | Add match model to the floor; measure ΔS and longest-match distribution | PROPOSED / UNVERIFIED (domain knowledge) |
| **P5** Word model | Token/word-boundary contexts predict text far better than raw orders | yes | text predictor | table bytes | model bytes | medium | high on enwik | Compare word-order contexts vs equivalent byte orders | PROPOSED / UNVERIFIED (domain knowledge) |
| **P6** Sparse / indirect models | Gapped contexts and indirect (context-of-context) histories capture structure | yes | text predictor | table bytes | model bytes | medium | medium | Ablate sparse and indirect model groups separately | PROPOSED / UNVERIFIED (domain knowledge) |
| **P7** Task-specific preprocessing (paq8px image/audio/text) | Invertible transforms lift model-relevant structure | yes | preprocessor library | transform flag; exact round-trip | 0 | low | medium | Port only text/markup transforms; verify round-trip | PROPOSED / UNVERIFIED (domain knowledge) |
| **P8** Tiny arithmetic-coder overhead | Payload dominates once models are good | yes | — | coder code in binary | code bytes | low | enables P1–P6 | Measure coder overhead in isolation | PROPOSED / UNVERIFIED (domain knowledge) |
| **P9** Nibble-bucketed hash tables with checksums | Cheap collision handling and locality for large contexts | yes | context store | table bytes | model bytes | low–medium | medium | Compare bucket/checksum sizes; measure collisions and ΔS | PROPOSED / UNVERIFIED (domain knowledge) |

**Build / measure.** This is Zentropy's Phase-3/6 predictive spine. Build a minimal PAQ-like
core (P1+P3+P4+P8) first, then ablate P2, P5, P6, P9 one at a time. Deliverable measurement:
bits/bit on an enwik9 slice for each cumulative model set, and the marginal ΔS of each.

---

## 7. CMIX lineage (cmix, fx-cmix, fx2-cmix, fx2-cmix-transformer, fx-deepmix, cmix-lex)

The CMIX lineage's transferable core is *scaling context mixing to hundreds of models under a
hard byte budget by trading executable size for prediction quality*, plus a set of concrete
enwik-specific transforms (stemming, reverse dictionary, article reordering, metadata hoisting).
The `fx2-cmix` and `starlit` READMEs were fetched; `fx-deepmix`, `cmix-lex`, and `zpaq` 404'd.
Variant tags: `[cmix]`, `[fx-cmix]`, `[fx2]`, `[fx2-tr]`, `[deepmix]`, `[cmix-lex]`.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **C1** `[cmix]` Large ensemble of heterogeneous context models + mixers + APMs | Ratio comes from many weak, complementary predictors | yes (GPL-family, license: verify) | predictor ensemble | all models/weights in binary | large model + code bytes | high (hours decode) | high | Scale ensemble on a slice; plot ΔS vs model count and vs binary bytes | PROPOSED / VERIFIED (source) for the ensemble's existence (fx2 README §"num models 461"); / UNVERIFIED for exact composition |
| **C2** `[cmix]` LSTM cell as a mixer/predictor | Recurrent learned state captures longer dependencies than order-N | yes (license: verify) | learned predictor | LSTM weights + code | weights + code | high | high | Ablate LSTM on/off; measure ΔS vs runtime | PROPOSED / VERIFIED (source: fx2 README notes LSTM expected-byte and prediction usage) |
| **C3** `[cmix]` PPMd model with mmap-to-disk option | A strong nonstationary byte model; mmap trades RAM for disk | yes | predictor + RAM compliance | model + code | model bytes | high | high | Compare PPMd RAM vs mmap; measure ΔS and peak RAM | PROPOSED / VERIFIED (source: fx2 README PPM `mmap_to_disk`) |
| **C4** `[cmix/fx]` Recursive enwik transform / dictionary preprocessing | Reversible rewriting before modeling | yes | preprocessor | transform logic, exact round-trip | 0 direct | low | medium | Verify round-trip; measure ΔS with transform on/off | PROPOSED / VERIFIED (source: fx2 README "single pass wikipedia transform") |
| **C5** `[fx-cmix/fx2]` Mixer weight-update skipping below an error threshold | Skip updates that cannot matter, buying runtime for more models | yes | speed lever | quantized update rule in binary | 0 | lower runtime | frees budget for C1 | Enable threshold; measure runtime saved and ΔS lost | PROPOSED / VERIFIED (source: fx2 README) |
| **C6** `[fx-cmix]` Delete slow/weak models to reallocate complexity | Remove 7 indirect nonstationary predictors, 6 match predictors, 3 mixers to make room | yes | architecture pruning | smaller binary | negative (smaller code) | lower | depends | Reproduce the pruning; confirm ΔS non-negative at equal runtime | PROPOSED / VERIFIED (source: fx2 README) |
| **C7** `[fx-cmix]` Reverse dictionary transform | Load the dictionary when first encountered; keep text and coded buffers separate | yes | dictionary side channel | dictionary + logic | dictionary bytes | low | medium–high | Ablate reverse-dictionary on/off; measure ΔS and dictionary size | PROPOSED / VERIFIED (source: fx2 README) |
| **C8** `[fx2]` Stemmer + word-type streams (4 streams, reset at sentence/paragraph; drop words by type) | Model word classes (Article, Conjunction, Adposition, …) as separate streams | yes | text model | stream logic in binary | 0 direct | low–medium | high | Implement word-type classification + streams; measure per-stream ΔS | PROPOSED / VERIFIED (source: fx2 README) |
| **C9** `[fx2]` Sparse match model (gap 1–2, min length 3–6) for escaped UTF-8 | Gapped matches catch UTF-8/markup escapes | yes | match model | model bytes | small | low | medium | Add gapped match; measure hits on escaped UTF-8 | PROPOSED / VERIFIED (source: fx2 README) |
| **C10** `[fx2]` Runtime-generated state tables | Generate state tables at decode to shrink the binary | yes | code-size lever | code only | negative (smaller) | tiny | free bytes | Replace tables with generators; confirm decode correctness and byte delta | PROPOSED / VERIFIED (source: fx2 README) |
| **C11** `[fx2]` ContextMap slot partitioning by memory tier (32/64/128 B) | Small contexts want small slots; large contexts want large slots | yes | memory/ratio tuning | table bytes | model bytes | medium | medium | Partition by context memory size; measure ΔS and RAM | PROPOSED / VERIFIED (source: fx2 README) |
| **C12** `[fx2]` Article reordering via 1024-dim embeddings → t-SNE to 1-D → sort → k-means clusters → reverse → manual sort | Reorder articles so similar content is adjacent, then restore by title sort | yes (verify license) | offline search (unbounded) | order file, compressed + embedded | compressed order file | negligible decode | medium–high (record-winning) | Reproduce order pipeline; measure ΔS of reorder on/off with title-sort restoration | PROPOSED / VERIFIED (source: fx2 README §"Article order") |
| **C13** `[fx2]` Single-pass Wikipedia transform (disk 18 GB → 7 GB, 7 min → 3 min) | Same semantics, less temp disk and time | yes | transform efficiency | same as C4 | 0 | lower | enables C4 at scale | Verify reduced temp disk stays under 100 GB and round-trip holds | PROPOSED / VERIFIED (source: fx2 README) |
| **C14** `[fx2-tr]` Replace LSTM with ~6 M-param transformer pretrained offline; feed classical PPM predictions as inputs; decoder CPU-only | Learned corrector consumes classical model predictions; weights embedded and counted | partial (license: verify) | offline GPU training | transformer weights + code | ≈2.9 MB extra binary + weights | CPU inference | high (record-pending) | Train on a slice; measure `residual_saved − weight_bytes − code_bytes` and CPU decode time | PROPOSED / VERIFIED (source: task baseline) |
| **C15** `[deepmix]` 2×200-cell LSTM, int8 kernels, grammar/revision-aware metadata modeling, hoist reconstructible Wikipedia fields, RSS-triggered PPMD eviction | Quantized recurrence + knowledge of Wikipedia metadata enables field hoisting and RAM control | partial | predictor + IR hoisting | model + hoist rules | model bytes | medium–high | medium–high | Implement field hoisting; verify exact reconstruction; measure ΔS and peak RSS | PROPOSED / VERIFIED (source: task baseline) |
| **C16** `[fx2/starlit]` Self-extracting construction: minify code, UPX-pack, compress embedded assets (dictionary, model, order file) with the compressor itself, append | `S1` counts the executable, so shrink code and pack assets | yes | packaging | construction script | reduces `S1` | none | direct `S1` reduction | Measure `S1` before/after UPX + minification; verify archive still self-extracts | PROPOSED / VERIFIED (source: starlit + fx2 READMEs) |
| **C17** `[cmix-lex]` Lexical-dictionary base used as the classical engine behind external transformer weights (altxs) | A lexical dictionary variant is strong enough to host a learned add-on | partial (license: verify) | base engine | dictionary + code | dictionary bytes | high | medium | Reconstruct the cmix-lex base on a slice; measure its standalone ΔS | PROPOSED / VERIFIED (source: task baseline; repo README unavailable) |

**Build / measure.** Zentropy's Phase-4/6/8 spine. Priority order for ablations: C12 (reorder,
cheap decode) → C4/C13/C7 (reversible transforms + dictionary) → C8/C9 (word/stream modeling) →
C15 (field hoisting) → C5/C6/C10/C11 (size/runtime levers) → C14 (learned corrector). Every row
must report exact round-trip and peak RAM on the judged path.

---

## 8. ZPAQ (including ZPAQL model-VM)

ZPAQ's transferable core is the *ZPAQL model virtual machine*: the context model is bytecode
stored in the archive and executed by a small decoder VM, so new models require no new decoder
binary. Secondary cores are a *journaling archive with dedup/versions* and a *method ladder*.
The README fetch 404'd; all rows `[DK]` (ZPAQ spec knowledge).

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **ZA1** ZPAQL model virtual machine | Predictor logic ships as bytecode in the stream; one decoder runs any model | yes (MIT/public-domain; verify) | model sandbox / experiment plane | VM interpreter in binary; model bytecode in stream | interpreter code + bytecode | medium (interpreted) | medium (research velocity) | Implement a minimal VM + one bytecode model; measure decode speed and ΔS vs native | PROPOSED / UNVERIFIED (domain knowledge) |
| **ZA2** CM primitives inside the VM (state machines, hashes, mixer) | The PAQ-style architecture is expressible as VM ops | yes | model sandbox | bytecode + tables | bytecode bytes | medium | medium | Encode a small CM model in bytecode; compare to native implementation | PROPOSED / UNVERIFIED (domain knowledge) |
| **ZA3** Journaling archive with dedup and versions | Archive remembers updates; identical spans stored once | yes | evidence/version plane | archive metadata | metadata bytes | low | low for enwik | Measure dedup ratio on enwik revisions | REJECTED for submission (not needed for exact single-file reconstruction) / UNVERIFIED (domain knowledge) |
| **ZA4** Method ladder (store / LZ77 / BWT / CM levels) | One format, escalating model strength | yes | harness ablation | method id bits | ~0 | varies | enables ablations | Add method selector to the research harness | PROPOSED / UNVERIFIED (domain knowledge) |
| **ZA5** Configurable per-block model + postprocessor | Give each block its own model and a reversible post-filter | yes | segmentation | block model + filter | model bytes | low | medium | Assign separate models to text vs markup blocks; measure ΔS | PROPOSED / UNVERIFIED (domain knowledge) |
| **ZA6** libzpaq API / self-describing method | Decoder reads its own model description | yes | integration | format header | header bytes | low | low | Adopt self-describing headers for the model config | PROPOSED / UNVERIFIED (domain knowledge) |
| **ZA7** Streaming / recovery / encryption | Robust long-running decoders | yes | operational | optional | small | low | zero for score | Defer; not score-relevant | REJECTED for v1 / UNVERIFIED (domain knowledge) |
| **ZA8** Per-block independent decode state | Bounds error propagation and RAM | yes | segmentation | block headers | small | low | medium | Compare per-block reset vs continuous state on ΔS and RAM | PROPOSED / UNVERIFIED (domain knowledge) |

**Build / measure.** The high-value transfer is ZA1/ZA2 as a *research sandbox* so new models
can be tried without rebuilding the submission binary, and ZA5/ZA8 as Phase-2/3 segmentation.
Measure VM decode overhead and whether VM-expressed models reach native ΔS.

---

## 9. NNCP (Bellard)

NNCP's transferable core is a *neural-network predictor driving an arithmetic/range coder*
(`-log2(p)` per symbol), with a fast C tensor library (`LibNC`) instead of PyTorch, plus
dictionary preprocessing of enwik markup and CUDA-accelerated training. The `nncp` page was
fetched; the model-internals rows remain domain knowledge.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **N1** Transformer predictor over bytes/tokens | Attention captures long-range text dependencies | yes (license: verify) | learned predictor | net weights + code | weights + code | CPU inference | high | Ablate transformer vs LSTM predictor on a slice; measure ΔS and CPU time | PROPOSED / VERIFIED (source: nncp page) |
| **N2** C tensor library (LibNC) for inference | Removes PyTorch dependency and its size overhead | yes (verify) | efficient inference | linked code | code bytes | faster CPU | enables N1 | Benchmark LibNC vs PyTorch size and speed | PROPOSED / VERIFIED (source: nncp page) |
| **N3** `-log2(p)` arithmetic coding of neural probabilities | Predictor emits probabilities; coder is model-agnostic | yes | coder interface | coder + code | code bytes | low | enables N1 | Verify the coder against exact reconstruction | PROPOSED / VERIFIED (source: task baseline) |
| **N4** Dictionary preprocessing of enwik markup/tags | Replace recurring markup strings with short codes before modeling | yes | preprocessor | dictionary bytes | dictionary bytes | low | medium | Ablate dictionary preprocessing; measure ΔS and dictionary size | PROPOSED / UNVERIFIED (domain knowledge) |
| **N5** CUDA acceleration for training | Training is a research-plane cost only | yes | research speed | none | 0 | none (decoder CPU) | enables N1 | Confirm no CUDA symbol is required on the decode path | PROPOSED / VERIFIED (source: nncp page CUDA download) |
| **N6** Online/adaptive model update during decode | Decoder can deterministically re-learn from the decoded prefix, so weights need not all ship | yes | decode contract | update code | code bytes (weights either shipped or regenerated) | high | medium | Measure ΔS when weights are regenerated vs shipped; verify determinism | PROPOSED / UNVERIFIED (domain knowledge) |
| **N7** Large embedding/vocabulary tokenization | Sub-word units shorten sequences and shrink the effective model | yes | tokenizer | vocab bytes | vocab bytes | low | medium | Compare byte vs sub-word tokenization on ΔS and vocab cost | PROPOSED / UNVERIFIED (domain knowledge) |
| **N8** PyTorch/GPU-required v2 path | Research convenience that violates the judged-run constraint if kept | no | research only | must not appear in judged binary | 0 | — | — | Confirm the judged build has no GPU dependency | REJECTED / VERIFIED (source: nncp page "GPU required") |

**Build / measure.** N1+N2+N3 are the Phase-8 learned-corrector recipe; N4 is a cheap Phase-4
preprocessor; N6 is the mechanism that decides whether weights count against `S`. Deliverable
measurement: ΔS and CPU decode time for a shipped-weights vs regenerated-weights transformer.

---

## 10. Transformer Hutter frontier (fx2-cmix-transformer, tufazip, altxs)

The frontier's transferable core is a *frozen, offline-trained transformer whose weights are
embedded and counted, consuming classical-model predictions and running CPU-only at decode*.
The counter-mechanisms (external weights, GPU in the judged run) are explicit rejects. All rows
derive from the task baseline; no external page was fetched for this section.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **T1** Frozen pretrained transformer embedded in the archive | Weights are part of `S`; training is offline and unconstrained | partial (license: verify) | offline training | weights + code count | weight bytes + code | CPU inference | high | Measure `residual_saved − weight_bytes − code_bytes` on a slice | PROPOSED / VERIFIED (source: task baseline) |
| **T2** Feed classical model (PPM) predictions into the transformer | The learned model corrects the residual of a strong classical predictor rather than learning text from scratch | partial | predictor cascade | interface + weights | weights | medium | high | Ablate PPM-feature input; measure ΔS vs transformer-only | PROPOSED / VERIFIED (source: task baseline; prompt.txt) |
| **T3** Replace LSTM with transformer at equal parameter budget | Attention beats recurrence for long-range enwik structure | partial | architecture swap | weights | ≈ same weights | CPU | high | Swap LSTM→transformer at fixed params; measure ΔS and time | PROPOSED / VERIFIED (source: task baseline) |
| **T4** GPU used only for training; judged run is CPU-only | Constraint-compatible use of acceleration | yes (discipline) | offline training | none | 0 | none | enables T1 | Assert the decoder binary links no GPU runtime | ADOPTED / VERIFIED (source) |
| **T5** CPU portability fix (segfault) yielding 96,994,188 vs 96,996,198 | Decoder robustness is worth real bytes | yes | portability | fixed code | code bytes | low | small but real | Rebuild on Intel and confirm the 2 KB improvement and exactness | PROPOSED / VERIFIED (source: task baseline) |
| **T6** Externally-supplied transformer weights (altxs) with GPU required in judged run | Smaller `S1` by pushing model data outside the archive | no | — | violates no-external-data and no-GPU rules | negative apparent `S1` | — | disqualifying | Reject; document as anti-mechanism | REJECTED / VERIFIED (source: task baseline) |
| **T7** CUDA preprocessor in the judged run (tufazip) | Fast preprocessing on GPU, but decoder must not need GPU | no | research only | violates no-GPU rule | — | — | disqualifying | Reject for judged path; allow encoder-only | REJECTED / VERIFIED (source: task baseline) |
| **T8** Net-gain gate: admit learned components only if `residual_saved > model_bytes + binary_bytes` | Makes the accounting decisive | yes (discipline) | admission rule | none | 0 | 0 | prevents negative transfers | Apply the gate in Phase 8 before any weight work | ADOPTED / VERIFIED (source) |
| **T9** Weight quantization / compression to shrink model bytes | Fewer scored bytes for the same predictor quality | partial | model compression | smaller weights | reduces weight bytes | CPU dequant | medium | Quantize weights (int8/int4); measure ΔS and accuracy loss | PROPOSED / UNVERIFIED (domain knowledge) |

**Build / measure.** Phase 8. Build the transformer only after T8's cheaper stages are measured;
report the gate arithmetic explicitly for every learned component.

---

## 11. STARLIT

STARLIT's transferable core is *offline article reordering whose decoder is only a cheap,
reversible title sort*, plus a set of cmix-tuning changes made to fit the Hutter budget. The
`starlit` README was fetched; this section is the best-grounded in the document.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **S1** Reorder articles to place similar ones adjacent, learned offline | Search for an order that minimizes the downstream archive size; search is unconstrained | yes (license: verify) | unbounded offline search | none | 0 (search not shipped) | low (reorder at decode) | medium–high | Reorder a slice by an unconstrained search; measure downstream ΔS vs original order | PROPOSED / VERIFIED (source: starlit README) |
| **S2** Restore original order by alphabetically sorting titles | Only a cheap, general sort is needed to invert an arbitrary permutation | yes | — | sort code only | negligible | low | enables S1 | Verify that sorting titles exactly restores enwik9 order on the slice | PROPOSED / VERIFIED (source: starlit README) |
| **S3** Doc2Vec feature vectors + TSP over articles | Similarity is defined in an embedding space, not by byte overlap | yes (PySpark; license: verify) | ordering search | none | 0 | encoder-only | medium–high | Compare Doc2Vec+TSP vs the fx2 embeddings→t-SNE→k-means pipeline on ΔS | PROPOSED / VERIFIED (source: starlit README) |
| **S4** Context-buffer-eviction hypothesis: reuse shared context before eviction | Ordering should maximize temporal locality of shared context | yes (concept) | ordering objective | none | 0 | 0 | explains S1 | Measure cache-hit/eviction stats of the predictor under reordering | PROPOSED / VERIFIED (source: starlit README) |
| **S5** Ship the order file, compressed with the compressor itself, embedded | The permutation is data and counts against `S`; compress it | yes | construction | order-file bytes | compressed order bytes | negligible | medium | Measure compressed order-file size vs ΔS it buys | PROPOSED / VERIFIED (source: starlit README) |
| **S6** cmix tuning for budget: disable PAQ8 model, disable layer-1 mixers, PPMD 850 MB, LSTM 1×200, constant LR, float instead of double, PGO | Reallocate compute/RAM toward the predictors that matter under the cap | yes | budget tuning | changes in binary | reduces code/RAM | lower | enables larger model | Reproduce tuning; verify RAM ≤ cap and ΔS | PROPOSED / VERIFIED (source: starlit README) |
| **S7** HP-2017 enwik transforms + `sed` guards to protect patterns from wrong transformation | Reversible enwik-specific rewrites plus guards against over-transformation | yes (verify) | preprocessor | transform code; exact round-trip | 0 direct | low | medium | Verify round-trip and measure ΔS with transforms on/off | PROPOSED / VERIFIED (source: starlit README) |
| **S8** Embed a compressed English dictionary in the executable | Reuses the CMIX word prior directly | yes | dictionary side channel | dictionary bytes | dictionary bytes | low | medium | Ablate embedded dictionary; measure ΔS | PROPOSED / VERIFIED (source: starlit README) |
| **S9** Ship only the learned order, not the search program | Decoder independence: search is encoder-only | yes (discipline) | — | none | 0 | 0 | enables S1–S3 | Enforce decoder independence in the format | ADOPTED / VERIFIED (source) |

**Build / measure.** Phase 7 & 10. Reproduce S1–S2 on a bounded slice first (cheap, decisive),
then S5's order-file accounting. Compare against C12's embedding pipeline; keep whichever has
the better (ΔS − order-bytes) per unit search cost.

---

## 12. DeSaxe and StarComp

Both names are `UNRESOLVED_REFERENCE`. The in-repo prior ledger
(`zentropy/docs/STACK_TECH_TRANSFER.md` §10) already records that discovery for **DeSaxe**
resolved only to an unrelated offset-crank engine concept, and for **StarComp** only to an
unrelated old SourceForge application. Per the task's context-budget rule, no additional
external search was performed here beyond that existing in-repo resolution; **no mechanism is
attributed** to either name, and none should be until the exact repository/paper is identified
and read.

| Mechanism | Semantic essence | Reusable code? | Research-plane role | Submission-plane role | Est. byte cost | Est. runtime cost | Expected information gain | Experiment required | Status |
|---|---|---|---|---|---|---|---|---|---|
| **D1** DeSaxe | Unidentified; do not attribute any mechanism | no (unknown) | unknown | unknown | unknown | unknown | unknown | Locate the exact compressor/repo/paper; read source before any claim | UNRESOLVED_REFERENCE |
| **D2** StarComp | Unidentified; do not attribute any mechanism | no (unknown) | unknown | unknown | unknown | unknown | unknown | Locate the exact compressor/repo/paper; read source before any claim | UNRESOLVED_REFERENCE |

---

## Cross-cutting candidate mechanisms for Zentropy

Ranked by expected (Hutter-score gain) ÷ (engineering cost × uncertainty × resource risk). Each
candidate bundles mechanisms already defined above and names the Phase where it enters. All are
`PROPOSED` unless marked; Phase-0/8 laws and rules are `ADOPTED`.

| Rank | Candidate (mechanisms) | Source grounding | Enters Phase | Byte cost (scored) | Engineering cost | Uncertainty | Resource risk | Net rationale |
|---|---|---|---|---|---|---|---|---|
| — | **Constitutional laws:** exact `decode(encode(X))==X`, universal `Literal` fallback, complete-cost accounting, decode-has-no-search-authority, net-gain gate, no external data, no GPU in judged run | STACK_TECH_TRANSFER §11; T4/T6/T7/T8 | Phase 0 | 0 | 0 | none | none | Free, blocking, and prevents invalid or disqualifying submissions. **ADOPTED.** |
| 1 | **Wikipedia article reordering with cheap title-sort restoration** (C12, S1, S2, S3, S4, S5) | fx2 README §"Article order" (**verified**); starlit README (**verified**) | Phase 7 & 10 | compressed order file only | medium (offline search, unbounded) | low (two independent winning systems) | low (decode is a sort) | Highest proven ratio per unit risk: offline search is free, decode is negligible, and it changes what every downstream model sees. |
| 2 | **Classical context-mixing spine** (P1, P2, P3, P4, P5, P8; C1, C2, C3) | PAQ8 domain knowledge; fx2 README (461-model ensemble, LSTM/PPMd) | Phase 3 & 6 | model + code bytes (dominant) | high | low–medium | medium (RAM) | The proven enwik-workhorse floor; everything learned should be measured as a residual on top of it, not instead of it. |
| 3 | **Transformed dictionary + match model + repeat-offset state** (B1, B5, Z2, Z3, Z5, Z6, X2, X6, P4, C7, C8, C9) | Brotli/Zstd/LZMA/PAQ domain knowledge; fx2 README (reverse dict, stemmer, sparse match) | Phase 4 | dictionary + model bytes | medium | low–medium | low | Largest well-understood, low-uncertainty gain; rep-state and word-class modeling are cheap to decode and repeatedly validated. |
| 4 | **Grammar + rank state with an entropy-coded skeleton** | STACK_TECH_TRANSFER §1, §11 (EntropyFS 12D-0/12D-1) | Phase 5 | grammar + state + residual | medium–high | medium–high | low | Only sibling-measured transfer result (−47% on grammar skeleton); high ceiling with real uncertainty. |
| 5 | **Residual-conditioned learned corrector / transformer over classical predictions** (N1, N2, N3, T1, T2, T3, T9; C14, C15) | nncp page (**verified**); task baseline (fx2-tr, deepmix); prompt.txt cascade | Phase 8 | weights + code (must pass net-gain gate) | very high | high | high (RAM/runtime) | Highest ceiling and hardest accounting; admitted only after cheaper stages and only if `residual_saved > model_bytes + binary_bytes`. |
| 6 | **Binary range coder for prediction-heavy streams** (X1, P8) | LZMA domain knowledge; ryg-rans-rs (per STACK_TECH_TRANSFER §8) | Phase 2 | coder + model bytes | low | low | low | Cheap primitive with a firm selection rule; emitted bytes decide vs rANS/FSE. |
| 7 | **FSE/tANS for finite side streams** (Z1, Z5, Z2) | Zstd domain knowledge | Phase 2 | table + coder bytes | low | low | low | Fast, compact entropy coding for alphabets, side channels, and sequence symbols. |
| 8 | **Reversible enwik preprocessors (single-pass wiki transform, HP-2017 + sed guards, field hoisting, BCJ/Delta)** (C4, C13, C15, S7, X8, N4) | fx2 README (**verified**); starlit README (**verified**); LZMA/NNCP domain knowledge | Phase 1 & 3 | 0 direct; transform logic | medium | low–medium | medium (temp disk) | Lifts model-relevant structure; single-pass changes make it fit the 100 GB temp budget. |
| 9 | **Article/metadata-aware IR hoisting** (C15, C12 interplay) | task baseline (deepmix) | Phase 1 & 3 | hoist rules + descriptors | medium | medium | medium | Reconstructible Wikipedia fields should not be modeled as text; exactness must be proven. |
| 10 | **Self-extracting construction and byte-shrinking levers** (C16, C5, C6, C10, C11) | fx2 README (**verified**); starlit README (**verified**) | Phase 0 & 8 (wiring) | reduces `S1`, no decode payload | medium | low | low | Direct `S` reduction and runtime headroom for more models; monotonically safe if exactness holds. |
| 11 | **ZPAQL-style model VM as a research sandbox** (ZA1, ZA2, ZA4, ZA5, ZA8) | ZPAQ domain knowledge | Phase 2 & 6 (research) | interpreter + bytecode if shipped | medium | medium | low | Speeds experimentation and ablation; only ship if VM decode cost is repaid by model quality. |
| 12 | **LZ4/LZ77 fast path and `A0` floor** (L1–L7) | LZ4 domain knowledge | Phase 0 (harness) | 0 (baseline) | very low | none | none | Negative control and speed fallback; not a ratio mechanism on enwik. **ADOPTED** as floor. |
| 13 | **BWT-family reordering** (W1–W9) | bzip2/bzip3 domain knowledge | Phase 10 (evidence-gated) | primary index + back-end | medium | medium | medium (temp disk) | Optional reordering of long-range-redundant spans; likely `SUPERSEDED` by direct CM, retained as an ablation. |
| 14 | **PPMd with mmap-to-disk** (C3) | fx2 README (**verified**) | Phase 6 | model bytes | low | high (disk) | high (SSD wear) | RAM-compliance lever; keep the non-mmap path as default per fx2 README recommendation. |
| 15 | **Journaling/dedup archive semantics** (ZA3, ZA7) | ZPAQ domain knowledge | — (deferred) | metadata bytes | medium | low | low | Not needed for exact single-file reconstruction; defer unless revision corpora are targeted. **REJECTED for v1.** |

**Gate mapping.** The ranking is decided empirically by the existing milestone gates: G0 exact
1 GB reconstruction, G1 beats generic compressors, G2 competitive with serious context mixing,
G3 beats the historical Hutter record, G4 beats the strongest credible pending frontier
(`fx2-cmix-transformer`), G5 exceeds the 1% prize threshold against the applicable predecessor,
G6 5% safety margin, G7 full rule/resource closure. Every candidate must be ablated from the
`A0` floor upward before it becomes `MEASURED` or `ADOPTED`, and any candidate outside Hutter
resource limits is `RESEARCH_ONLY` regardless of byte savings.

---

## Verified sources

Fetched 2026-09-11 (three successful; compact pages only, per the context-budget rule):

1. `https://raw.githubusercontent.com/kaitz/fx2-cmix/master/README.md` — grounds C1, C2, C3, C4,
   C5, C6, C7, C8, C9, C10, C11, C12, C13, C16; and the official fx2-cmix result table
   (`S1`=441,463, `S2`=110,351,665, `S`=110,793,128, `L`=112,578,322, 1.585%), 461 models,
   PPMd mmap, 65 h decode, ~9.52 GB peak RAM, ~21 GB disk, Xeon 3.10 GHz / Geekbench 1026.
2. `https://raw.githubusercontent.com/amargaritov/starlit/master/README.md` — grounds S1–S9
   (article reordering, alphabetical title-sort restoration, Doc2Vec+TSP, unbounded offline
   search, context-eviction hypothesis, embedded compressed order file, cmix tuning, HP-2017
   transforms, embedded dictionary) and the STARLIT result table (`S1`=390,308, `S2`=114,904,928,
   `S`=115,295,236, 1.81% AMD / 1.763% Intel).
3. `https://bellard.org/nncp/` — grounds N1, N2, N5, N8 and the result table
   (NNCP 2023-10-21: enwik8 14,915,298 B = 1.19 bpb; enwik9 106,632,363 + 628,955 program =
   107,261,318 total; CMIX v19 111,470,932 + 223,485 = 111,694,417).

**In-repo verified baseline (not re-fetched).** The pending frontier and anti-mechanism numbers
(`fx2-cmix-transformer` 96,996,198 / 96,994,188 / ≈100,424,672 / +2.9 MB / 6 M params / 8×RTX5090
26 h; `tufazip` 105,924,360 + 54,709; `altxs` 93,434,410 + 13,490,401; `nncp v3.2`
106,632,363 + 628,955; `fx-deepmix` 107,828,411) are taken from the task brief and
`zentropy/prompt.txt`. The unresolved-reference resolution is taken from
`zentropy/docs/STACK_TECH_TRANSFER.md` §10.

**Fetch attempts that failed (404), abandoned per the context-budget rule:**
`https://raw.githubusercontent.com/kaitz/fx-deepmix/master/README.md`,
`https://raw.githubusercontent.com/kaitz/cmix-lex/master/README.md`,
`https://raw.githubusercontent.com/zpaq/zpaq/master/README.md`. The affected rows (C15, C17,
ZA1–ZA8) are marked `UNVERIFIED (domain knowledge)` or cite the task baseline.

---

## Unverified / unresolved

**Unresolved references (no mechanism attributed):**

- **DeSaxe** (`D1`) — `UNRESOLVED_REFERENCE`. In-repo ledger records discovery resolving only to
  an unrelated offset-crank engine concept.
- **StarComp** (`D2`) — `UNRESOLVED_REFERENCE`. In-repo ledger records only an unrelated old
  SourceForge application.

**Unverified (domain knowledge) — mechanism asserted but not read from a fetched source:**

- All of §1 Brotli (B1–B8), §2 Zstandard (Z1–Z9), §3 LZ4 (L1–L7), §4 XZ/LZMA2 (X1–X9),
  §5 Bzip3/BWT (W1–W9), §6 PAQ8 (P1–P9), §8 ZPAQ (ZA1–ZA8).
- Within §7 CMIX: C15 (`fx-deepmix`) and C17 (`cmix-lex`) rest on the task baseline, not a read
  README; C1–C14 are `VERIFIED (source)` for the fx2-cmix README's claims, but the upstream
  *internals* of cmix/fx-cmix not present in that README remain domain knowledge.
- Within §9 NNCP: N4, N6, N7 are domain knowledge; N1, N2, N5, N8 are `VERIFIED (source)`.
- Within §10: T1–T8 are `VERIFIED (source)` against the task baseline; T9 is domain knowledge.
- Within §12: both rows are `UNRESOLVED_REFERENCE`, not domain knowledge.

**Licensing caveat.** Only Brotli (MIT), Zstandard (BSD/GPLv2 dual), LZ4 (BSD-2), and the
LZMA SDK (public domain) licenses are asserted with confidence. Every other `Reusable code?`
cell marked `license: verify` must be checked against the Hutter Prize's OSI-license
requirement before code is vendored.
