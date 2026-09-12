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
`2^bits * 4` bytes of RAM (64 MB at enwik9). The **enwik9 gate is running** and no
adoption is made before it (A29: the enwik8 screen has reversed twice already in
this project).
