# Phase 4 — Dictionary + match + repeat-offset state

> Deliverable (architecture §6): **transformed lexical/phrase dictionary, long/sparse
> matches, repeat references** (repeat-offset state).
>
> Prior-art bundle (PRIOR_ART_MECHANISMS.md rank 3): `B1, B5, Z2, Z3, Z5, Z6, X2,
> X6, P4, C7, C8, C9`. Zentropy is a context-mixing predictor, not an LZ77 codec,
> so each LZ-sequence mechanism is *adapted* to the CM: "match/rep state" becomes
> match-family experts and mixer selector state; "dictionary" becomes reversible
> lexical transforms plus the stored corpus-derived vocabulary.

## Method (non-negotiable, from the constitution)

Every item below is executed in order and receives:

1. an **exact, reversible** implementation behind a feature flag (so the accepted
   build is untouched and the mechanism is ablatable);
2. an exhaustive **round-trip court** on fixtures, malformed input and random data;
3. a **hostile negative control** that destroys the signal the mechanism claims to
   exploit (A32);
4. a **measured marginal executable cost** (`tools/measure_binary_cost.sh`, in the
   accepted-reachable configuration — A31);
5. a **ladder measurement** (`enwik6`, `enwik7`, `enwik8`, and `enwik9` before any
   adoption), each compared against the *parent*, with a receipt;
6. a documented verdict: `ADOPTED`, `REJECTED`, or `REJECTED_SMALL_ADOPTED_LARGE`.

No item is skipped, picked, or deferred. A rejection with a control is a
first-class result.

## Ordered sequence

### Match + repeat-offset state

| # | Item | Ledger | What is built |
|---|---|---|---|
| 4.1 | Long-distance / length-stratified match | Z6, A6 | a second match tier with a longer minimum and its own index, capturing far repeats the short tier evicts |
| 4.2 | Sparse / gapped match | C9 | matches over a gapped context (gap 1–2, min 3–6) for escaped UTF-8 and markup |
| 4.3 | Repeat-offset state | B5, Z3, X6, A7 | an MRU ring of recent match distances; rep-source predictors; rep-state mixer selector |
| 4.4 | Matched-literal residual | X2, X3, A8 | literal conditioned on the matched source byte, including a delta path |
| 4.5 | Distance-conditioned match floors | A9, X5 | minimum match length by distance class; distance-class context |

### Transformed lexical/phrase dictionary

| # | Item | Ledger | What is built |
|---|---|---|---|
| 4.6 | Stemming / root+affix transform | B1, C8 | reversible morphological transform (stem + affix code) |
| 4.7 | Word-type streams | C8 | word-class classification as a context expert / stream |
| 4.8 | Transformed dictionary entries | B1 | prefix/suffix/case transform representation for vocabulary entries |
| 4.9 | Phrase dictionary | A26, A27 | frequent multi-word phrases as tokens, not just single words |
| 4.10 | Dictionary entry compression | B1, C7 | front-coding of the stored vocabulary (shared prefixes) |
| 4.11 | Reverse-dictionary accounting | C7 | verify/record how the stored vocabulary already satisfies the reverse-dictionary mechanism |

`Z2`/`Z5` (separate literal-length/match-length/offset distributions and entropy
table modes) have no LZ parser to attach to in a CM; their semantic content —
"each sequence element is a separate distribution" — is carried by 4.3 and 4.5
(offset class and match-length class exposed as mixer selector state).

## Results

### 4.1 Long-distance / length-stratified match

A second match tier with its own index and a longer minimum. The short tier is
unchanged (`MATCH_MIN = 6`); the long tier is added as a separate mixer input.

Dose-response (archive delta vs the accepted parent `column-word-token-reverse`):

| tier min | enwik7 archive Δ | enwik8 archive Δ |
|---|---|---|
| 6 (redundancy control) | **+5,143** | **+57,012** |
| **8** | **−4,229** | **−77,348** |
| 12 | −2,632 | −27,874 |
| 16 | −393 | — |
| 24 | −464 | — |

The redundancy control (a second tier with the *same* minimum as the short one)
**hurts** at both rungs, while longer minima help and peak at 8 — so the gain is
the length/distance separation, not merely "one more match input".

Measured marginal executable cost in the adopted configuration (accepted method
set to `long-match8`): **1,240 B** (submission stub 347,448 vs 346,208).

```
enwik8 parent    column-word-token-reverse  22,181,992  1.7746 bpc  exact
      candidate long-match8               22,104,644  1.7684 bpc  exact
      archive_delta -77,348 ; charged 1,240 B ; DeltaS -76,108  -> ADOPTED (enwik8)
```

Encode time rises ~31% (91.6 s -> 120.1 s on enwik8) and the long tier costs
`2^bits * 4` bytes of RAM (64 MB at enwik9). The **enwik9 gate**:

```
enwik9 parent    column-word-token-reverse  180,079,678  1.4406 bpc  exact
       candidate long-match8               178,623,557  1.4290 bpc  exact
       archive_delta -1,456,121 ; charged 1,240 B ; DeltaS -1,454,881 -> ADOPTED
```

### 4.2 Sparse / gapped match

A match tier whose context ends `gap` bytes before the current position, so it
catches repeats whose immediately preceding byte(s) differ (ledger C9).

| (min, gap) | enwik7 archive Δ | enwik8 archive Δ |
|---|---|---|
| (4, 1) | **−5,636** | **−30,993** |
| (6, 1) | +3,283 | — |
| (6, 2) | +5,518 | — |

Short sparse contexts recur; long gapped ones rarely do and act as noise (the
same failure as 4.1's redundancy control). `sparse-match4g1` is positive at both
rungs. Binary cost and the enwik9 gate are pending, together with 4.1.

### 4.3 Repeat-offset state

An MRU ring of recent match distances feeding 1–4 repeat-offset predictors, whose
confidence adapts on hit/miss (ledger B5/Z3/X6).

| depth | enwik7 archive Δ | enwik8 archive Δ |
|---|---|---|
| 1 | −258 | — |
| 2 | −1,880 | — |
| **3** | **−2,021** | **+29,105** |
| 4 | −1,656 | — |

**REJECTED — scaling reversal.** The mechanism helps at enwik7 and hurts at
enwik8. Caveat (A32): the rep predictors also add three mixer inputs, and mixer
input-count changes can move the archive on their own; a null-input control would
separate dilution from signal. The reversal makes the point moot for adoption.

### 4.4 Matched-literal expert

An expert whose context is the best active match tier's predicted byte plus its
match state (ledger X2/X3). Its control (`match-byte-const`) uses the same expert
with the predicted byte replaced by a constant, isolating the value of the
prediction from that of one more mixer input.

| variant | enwik7 archive Δ | enwik8 archive Δ |
|---|---|---|
| `match-byte` | **−43,858** | **−333,917** |
| `match-byte-const` (control) | −1,280 | −12,219 |

The predicted byte carries almost all of the signal (≈34x the control at
enwik7). The **enwik9 gate runs with the 4.1+4.2+4.4 composite** (`phase4`).

### 4.5 Distance-conditioned match floors

Far matches must be longer to earn the same confidence (ledger A9/X5).

| corpus | archive Δ |
|---|---|
| enwik7 | +28,713 |
| enwik8 | +231,163 |

**REJECTED** at both rungs. Discounting distance this crudely damages the long
tier the 4.1/4.4 wins depend on.

### 4.6 Stem / root+affix transform

A reversible morphological production (`MARK code stem`). Standalone vs the floor:

| corpus | archive Δ |
|---|---|
| enwik7 | +22,934 |
| enwik8 | +243,672 |

**REJECTED.** Consistent with A1.2 and A26: normalising surface forms destroys
distinctions the CM exploits, and the marker bytes are not recovered.

### 4.7 Word-class context

A closed-class word-type expert (ledger C8), with a constant control.

| variant | enwik7 archive Δ |
|---|---|
| `word-class` | −1,226 |
| `word-class-const` (control) | −1,055 |

The closed-class information adds only ≈171 B over the extra mixer input, i.e.
nothing. **REJECTED** — redundant with the word and word-bigram experts.

### 4.8 Affix-referenced token entries

A word not in the vocabulary encoded as `(base id, affix code)` (Brotli-style
dictionary transform, ledger B1). enwik7 archive Δ **+8,989** -> **REJECTED**.

### 4.9 Phrase (multi-word) vocabulary

Frequent adjacent-word phrases as two-byte tokens.

| variant | enwik7 archive Δ | enwik8 archive Δ |
|---|---|---|
| `column-word-token-phrase` (reverse ids) | +7,077 | +51,531 |
| `column-word-token-phrase-freq` | +15,422 | — |

**REJECTED.** Phrases displace single-word tokens from the 255-entry budget, and
the CM already models adjacent words.

### 4.10 Front-coded dictionary header

The stored vocabulary front-coded against the previous entry. enwik7 archive Δ
**+20** — the dictionary is ~2 KB and already entropy-coded in-stream, so
front-coding is neutral. **REJECTED** (within noise, and it adds code).

### 4.11 Reverse-dictionary accounting

The ledger's C7 reverse-dictionary transform ("load the dictionary when first
encountered") is **already satisfied** by the adopted A1.1 design: the
corpus-derived vocabulary is stored as a prefix of the same modelled stream and
parsed by the decoder before the body, so text and coded buffers are separate and
the dictionary is charged. No new mechanism is required; this item is
`satisfied by A1.1` rather than a separate implementation.

## Verdict summary

| item | ledger | verdict |
|---|---|---|
| 4.1 long-distance match | Z6 | **ADOPTED** (enwik9 DeltaS −1,454,881) |
| 4.2 sparse match | C9 | positive (in the gate composite) |
| 4.3 repeat-offset state | B5/Z3/X6 | REJECTED (enwik8 reversal) |
| 4.4 matched-literal | X2/X3 | **positive** (in the gate composite) |
| 4.5 distance floors | A9/X5 | REJECTED |
| 4.6 stem transform | B1/C8 | REJECTED |
| 4.7 word-class context | C8 | REJECTED |
| 4.8 affix-referenced entries | B1 | REJECTED |
| 4.9 phrase vocabulary | A26/A27 | REJECTED |
| 4.10 front-coded dictionary | B1/C7 | REJECTED (neutral) |
| 4.11 reverse dictionary | C7 | satisfied by A1.1 |
