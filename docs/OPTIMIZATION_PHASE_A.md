# Optimization Phase A — Untapped Delta Campaign

> Locate and exploit compression mechanisms **not already covered** by the
> existing Zentropy architecture. This is not a feature-collection phase; it is
> an empirical search for `ΔS < 0` against a frozen parent, where `S` is the
> fully accounted Hutter score.
>
> **Make it informative, not successful.** Negative results are first-class.
>
> **Historical phase record.** This document describes Optimization Phase A as it
> was run. The accepted configuration has moved on since: it is now
> `Method::Residual` at `tune 5` (mixer LR 16, APM axis off), and every `tune 7`
> reference below is the A20-adopted value at the time of the phase, superseded by
> Phase 9. Likewise the `315,760 B` stub figure is the A0 parent, not the current
> artifact (**111,888 B**, see [`RESOURCE_CLOSURE.md`](RESOURCE_CLOSURE.md) §7). Read the
> phase's *measurements and controls* here; read
> [`RESEARCH_STATUS.md`](RESEARCH_STATUS.md) for what is true now.

## A0 — Frozen parent

Bound in `evidence/optimization-a/PARENT.json`:

| Field | Value |
|---|---|
| revision | `57ff6a8…` (later commits add Phase-A scaffolding) |
| toolchain | `rustc 1.98.0 (88d9e12ae 2026-08-18)` |
| submission stub | 315,760 B (`submission` profile, stripped) |
| driver | 429,872 B (`release`) |
| enwik6/7/8/9 | digests in `evidence/baseline/CORPUS.sha256` |

No Optimization-A decision uses an estimated binary cost. Every mechanism's
executable cost is produced by `tools/measure_binary_cost.sh`, which builds
otherwise-identical submission binaries with and without the feature.

**Measured marginal executable costs** (compiler interactions are real and are
charged; the value is measured in the all-features configuration):

| Mechanism | Measured marginal cost |
|---|---|
| `struct-hoist` (Phase 3) | 1,712 B |
| `alphabet-perm` (A2) | 472 B |
| `info-inherit` (A3) | 64 B |
| `column-model` (A17) | 720 B |
| `case-model` (A1.2, incl. composite methods) | 976 B |
| `word-token` (A1.1/A26, incl. composite methods) | **21,848 B** |

> **A31, learned the hard way.** The first `word-token` measurement read **576 B**
> because `ACCEPTED_METHOD` was still `Column`: with `lto = "fat"` the optimizer
> proved the token transform unreachable from the scored entry point and
> **dead-code-eliminated** it. Once the accepted method actually uses tokens the
> same measurement is **21,848 B** (the `std::HashMap` machinery for the word
> count), and it is consistent across feature sets. A mechanism's executable cost
> must therefore be measured with the mechanism *reachable from the accepted
> method*, not merely compiled in. The three earlier enwik7/enwik8 receipts that
> charged 576 B were re-issued at 21,848 B.

## Mechanism registry

Every mechanism is a `Method` variant, so each is individually ablatable and the
interaction matrix (A28) can be built by adding variants. `tune` is an orthogonal
runtime variant carried in the archive header, so optimizer variants cost **zero**
executable bytes.

| Method | Mechanism | Status |
|---|---|---|
| `rawcm` | floor only | baseline |
| `rawcm-noword` | floor minus word experts | ablation |
| `struct-hoist` | Phase 3 | ADOPTED (≥ enwik7) |
| `alphabet-perm` | A2 frequency permutation | **REJECTED** |
| `alphabet-random` | A2 negative control | control |
| `info-inherit` | A3 inheritance | **REJECTED** |
| `info-unrelated` | A3 negative control | control |
| `alphabet-perm+info-inherit` | A2+A3 | **REJECTED** |
| `tune 0..15` | A20 mixer update-law variants | **tune 7 ADOPTED** |
| `column` | A17 aligned previous-line/column expert | **ADOPTED** |
| `column-shuffled` | A17 wrong-column control | control |
| `column-noline` | A17 no-vertical null control | control |
| `case` | A1.2 case merge (no hoist) | screening only |
| `case-mark` | A1.2 mark-only (no hoist) | screening only |
| `column-case` | A1.2 merge on the accepted parent | **REJECTED** (enwik8 reversal) |
| `column-case-mark` | A1.2 mark-only on the accepted parent | **REJECTED** (enwik9 reversal) |
| `word-token` | A1.1/A26 frequency-ranked vocabulary | screening |
| `word-token-reverse` | A1.1/A26 reverse-id control | screening |
| `column-word-token` | A1.1/A26 frequency-ranked on the accepted parent | ADOPTED enwik8 |
| `column-word-token-reverse` | A1.1/A26 reverse ids on the accepted parent | **ADOPTED (accepted configuration)** |
| `word-token2` / `column-word-token2` | A26 escape-extended vocabulary (65,534 cap) | **REJECTED** (+776,196 at enwik8) |

Rejected/experimental mechanisms are **not** in the default (scored) build; they
reproduce with
`--no-default-features --features struct-hoist,alphabet-perm,info-inherit,column-model`.

## Results

### A17 — previous-line / structural-column expert (ADOPTED)

The expert's context is the byte at the aligned column of the previous line, plus
the byte to the left and a column bucket. Two controls isolate the signal:
`column-shuffled` takes a deliberately *wrong* column (still vertical), and
`column-noline` drops the previous-line byte entirely (left + column only).

| Method | enwik7 ΔS |
|---|---|
| `column` (aligned) | **−8,689** |
| `column-shuffled` (wrong column) | −4,983 |
| `column-noline` (no vertical byte) | −3,445 |

Dose-response is clean: no-vertical −3,445, misaligned −4,983, aligned −8,689.
Vertical alignment is worth ~5.2 KB, and the column bucket/left context alone is
worth ~3.4 KB. enwik8 confirmation:

```
parent    struct-hoist         22,372,738  1.7898 bpc
candidate column             22,313,281  1.7851 bpc
archive_delta = -59,457 bytes; measured marginal cost 720 B; DeltaS = -58,737
```

Accepted configuration is now **`struct-hoist + column expert + tune 7`**.

### A20 — optimizer update-law sweep (ADOPTED)

Learning-rate sweep, zero executable cost. enwik7 screening:

```
tune  0 (lr  12):  2,479,080  1.9833 bpc
tune  7 (lr  24):  2,469,588  1.9757 bpc   <- optimum
tune  8 (lr  32):  2,470,625  1.9765 bpc
tune 15 (lr 256):  2,653,193  2.1226 bpc
```

enwik8 confirmation (A29: enwik7 is a screening court, not authority):

```
parent    struct-hoist (tune 0):  22,449,221  1.7959 bpc
candidate struct-hoist (tune 7):  22,372,738  1.7898 bpc
DeltaS = -76,483 bytes at 0 measured executable cost   -> ADOPTED
```

The accepted configuration is now `struct-hoist + tune 7`. `archive::encode`
uses `ACCEPTED_METHOD`/`ACCEPTED_TUNE`; the production path (bench, compress,
SFX) reflects it automatically.

**Accepted-configuration artifact accounting.** The accepted submission stub is
**318,200 B**, i.e. **+2,440 B** over the frozen parent (315,760 B). That delta
is Phase-A *framework scaffolding* (the `Method` registry, the `tune` header
byte and plumbing, and the feature-off passthrough functions) — not a mechanism
gain. It is charged, not hidden, and is a Phase-11 reclaim target. Every
mechanism's own ΔS above was measured with identical scaffolding on both sides,
so no mechanism's verdict is affected by it.

> **A29 discipline.** enwik9 has **not** been re-run under the accepted
> configuration, and the enwik8 saving is **not** extrapolated to it. A full
> enwik9 run is the next milestone gate.

### A1.1 / A26 — dynamic word vocabulary (ADOPTED)

Unlike `struct-hoist` (a fixed 31-entry table in `.rodata`, zero archive
metadata), this vocabulary is **derived from the input** and must be *stored* in
the archive and charged. The dictionary is written as a prefix of the same
modelled stream, so the predictor entropy-codes it and its cost is fully
accounted for. Body encoding is injective:

```
token          = 0x00 id          (id in 1..=255, two bytes)
literal 0x00   = 0x00 0x00        (id 0 is never a token)
other byte     = copied verbatim
```

Vocabulary construction: candidate words have length >= 3 and count >= 2 and
survive only if a two-byte token recovers more than the entry's raw definition
cost; survivors are ordered by descending frequency (ties by word), truncated to
the 255-entry id space, and written as `count, (len, bytes)*`. The transform runs
after hoisting and case (`hoist -> case -> token -> perm`). Feature `word-token`,
measured scored-configuration marginal cost **21,848 B** (see the A31 note above).
`word-token-reverse` selects the same 255 words but assigns ids in *reverse*
frequency order (least frequent word gets id 1); it is the control that isolates
the value of frequency ranking from substitution itself.

Archive deltas are cost-independent; ΔS charges the 21,848 B:

| corpus | variant | parent | archive Δ | ΔS | verdict |
|---|---|---|---|---|---|
| enwik6 | `word-token` | `rawcm` | +1,394 | +23,242 | REJECTED |
| enwik6 | `word-token-reverse` | `rawcm` | +563 | +22,411 | REJECTED |
| enwik7 | `word-token` | `rawcm` | −7,043 | +14,805 | REJECTED |
| enwik7 | `word-token-reverse` | `rawcm` | −12,556 | +9,292 | REJECTED |
| enwik7 | `column-word-token` | `column` | +3,744 | +25,592 | REJECTED |
| enwik7 | `column-word-token-reverse` | `column` | −4,965 | +16,883 | REJECTED |
| enwik8 | `column-word-token` | `column` | −70,763 | −48,915 | ADOPTED |
| enwik8 | `column-word-token-reverse` | `column` | **−131,289** | **−109,441** | ADOPTED |
| enwik9 | `column-word-token-reverse` | `column` | **−1,723,929** | **−1,702,081** | **ADOPTED** |

**Finding 1 — the mechanism is `REJECTED_SMALL_ADOPTED_LARGE`.** Every rung
through enwik7 is rejected once the 21,848 B fixed cost is charged; the archive
delta turns positive at enwik8 and strongly positive at enwik9. This is precisely
the pattern A29 exists to protect: the small-rung rejects are correct *for the
small rung*, and only the full-corpus run decides.

**Finding 2 — frequency ranking is the wrong objective.** The reverse-order
control beats frequency-ranked ids at every rung that matters (enwik8: −131,289
vs −70,763 archive; enwik9 uses the reverse variant). This reproduces A2 in a
different mechanism: for a bitwise MSB-first predictor, raw symbol frequency is
not the right objective, so FOT-style variable-length frequency ranking is not
what a context-mixing backend wants.

**Finding 3 — large overlap with existing mechanisms (A28).** The same variant
saves 12,556 B on enwik9-scale text standalone but only 4,965 B on top of
hoist + column at enwik7 (and *loses* there once cost is charged), i.e. at small
scale it mostly removes redundancy the classical stack already captured. Its
enwik9 value comes from the long tail of repeated words that direct contexts
cannot reach.

**Adopted.** The accepted configuration is now
`struct-hoist + column expert + word-token-reverse + tune 7`:

```
enwik9   column (previous accepted)      181,803,607  1.4544 bpc
        column-word-token-reverse       180,079,678  1.4406 bpc   exact=true
        archive_delta -1,723,929 ; charged cost 21,848 ; DeltaS -1,702,081
```

The scored submission stub is now **346,208 B** (was 324,360 B): +21,848 B is the
measured token-transform cost, charged and kept. Eliminating `std::HashMap` (a
sorted-run or custom open-addressing counter would do) is a concrete Phase-11
size target worth ~20 KB.

> **Open caveat.** The causal story for reverse ordering is not established — it
> may be an artefact of the id byte's interaction with the CM's bit tree rather
> than a genuine ranking effect. A control assigning ids by an unrelated criterion
> (word length, say) would separate "ordering" from "substitution". Also, the
> transform's word-count `HashMap` raised measured enwik9 peak RSS to
> 4,671,049,728 B (4.35 GiB); the 4.41 GiB `projected_encode` still covered it,
> but with only ~60 MB of margin, so an explicit transform-aux term in
> `memory::projected_encode` is the robust follow-up.

### A1.2 — case factorization (merge REJECTED; mark-only screening-positive)

A1.2 separates lexical identity from orthographic case. The transform is applied
**after** structural hoisting (`hoist → case → perm`): hoisting replaces structural
strings with one-byte codes in `0x01..=0x1F`, and case markers reuse the low bytes
`0x00..=0x03`. Running case second is what keeps the two from colliding — the only
hoist codes case must escape are `0x01/0x02/0x03` (`<page>`, `</page>`, `<title>`),
which are rare (≈4 KB on enwik7), while every case marker costs exactly one byte.
Both transforms remain exact bijections; `decode` unhoists after uncasing.

Two variants and their control, `column-case` = hoist + column + merge (lower-case
and mark `Title`/`UPPER`/`MIXED`), `column-case-mark` = hoist + column + mark only
(identity preserved). Marker incidence on enwik7: 339,833 marked words
(`Title` 321,318 / `UPPER` 15,385 / `MIXED` 3,130).

| corpus | `column-case` (merge) | `column-case-mark` (mark only) |
|---|---|---|
| enwik6 | −985 **ADOPTED** | +9 **REJECTED** (fixed cost on a small archive) |
| enwik7 | −7,811 **ADOPTED** | −7,065 **ADOPTED** |
| enwik8 | **+5,379 REJECTED** | −10,062 **ADOPTED** |
| enwik9 | not run (merge already rejected) | **+427,246 REJECTED** |

Measured marginal cost `case-model` = **976 B**; all ΔS above charge it.

**Finding 1 — merging lexical identity reverses sign at scale.** The merge variant
wins on enwik6/enwik7 and *loses* on enwik8. Lower-casing `The → the` collapses
distinct forms the predictor was already exploiting cheaply, and the loss grows
with corpus size. This is the WRT warning reproduced on Zentropy: a substitution
that helps a weak backend can hurt a stronger one. `column-case` is **REJECTED**
per the scaling-reversal hard stop (A29/A40).

**Finding 2 — marking alone also reverses at scale.** `column-case-mark` keeps the
word verbatim and inserts one `0x01/0x02/0x03` byte before each marked word. It
loses 9 B on enwik6 (fixed cost dominates a small archive), wins at enwik7 and
enwik8, then **loses 427,246 B on enwik9**. A 10 KB enwik8 win became a 427 KB
enwik9 loss, so **A1.2 is REJECTED in full at the full-corpus authority**. The
mechanism is not rescued by any smaller-rung result.

The enwik9 run also produced the **first enwik9 number under the accepted
configuration** (`hoist + column + tune 7`):

```
parent    column      181,803,607 bytes  1.4544 bpc  1374.7 s  exact=true
```

that is **1,145,597 B (0.63 %)** better than the earlier pre-column enwik9 run
(182,949,204 B, 1.4636 bpc). It is the authoritative full-corpus baseline.

> **A29 lesson, measured.** Extrapolating the enwik8 result for mark-only would
> have predicted a win at enwik9 and produced a 427 KB regression. This is why
> enwik9 is the only final authority and why the mechanism was kept out of the
default build until the run completed.

> **Superseded control (A32).** The mark-only variant was intended as the
> dose-response control for the *merge* claim, and it did falsify that claim. The
> case-independent marker control is now moot for adoption (the family is
> rejected), but the observation stands: in this architecture, inserting cheap
> markers at word starts can help a medium corpus and hurt a large one, so any
> future boundary-marker mechanism must be gated on enwik9, never on enwik8.

### A26 — v2 escape-extended vocabulary (REJECTED)

v1 addresses only 255 tokens. v2 keeps two-byte tokens for the 254 most common
words and adds an escape-extended three-byte id (`0x00 0xFF hi lo`) for the long
tail, so the vocabulary can grow to 65,534 entries. New methods `word-token2`,
`column-word-token2`, feature `word-token2`. The accepted v1 baseline is
untouched (verified: the accepted enwik6 archive is unchanged at 272,066 B).

Archive deltas vs the *accepted* v1 tokenizer (cost-independent):

| corpus | cap | `column-word-token2` vs `column-word-token-reverse` |
|---|---|---|
| enwik7 | 65,534 | **+101,980** REJECTED |
| enwik8 | 65,534 | **+776,196** REJECTED |
| enwik7 | 1,024 | **+25,228** REJECTED |

**Finding — more coverage is not better; the optimum is at or below 255.** The
loss is monotone in the vocabulary cap, and dictionary size explains it: the
65534 cap admits 33,521 words (305 KB raw) on enwik7 and hits the cap on enwik8
(563 KB raw, compressed inside the modelled stream). v1's 255-entry dictionary
costs ~2 KB. The per-word admission test (`count*(len-code_len) - (len+1) > 0`)
is a *raw byte* profitability heuristic, and — exactly as A2 and A1.1's ordering
result already warned — raw-byte accounting is not authority for a context-mixing
backend: it ignores dictionary/model capacity, the loss of the replaced word's
letters as context, and the fact that the CM already models the long tail well.
Selecting vocabulary by *measured* archive impact (coder-in-the-loop) is the only
sound version of A27, and it is expensive.

A1.1 therefore stands at its 255-token v1 form, which is the adopted one.

### A2 — bitwise alphabet geometry (REJECTED)

| Method | enwik7 archive | ΔS vs parent |
|---|---|---|
| `alphabet-perm` (frequency) | +120,839 archive | **+121,311** |
| `alphabet-random` (control) | +171,558 archive | +172,030 |

Both permutations are worse than identity, but the **control differs strongly**
(freq is ~50 KB better than random). The mechanism is therefore real — the
predictor *is* sensitive to symbol labelling, as expected for a bitwise MSB-first
tree — but the frequency heuristic is the wrong objective.

**Finding (informative).** The correct objective is
`actual_range_coder_bytes(π(input))`, not symbol frequency or Shannon entropy.
Identity already encodes useful structure (ASCII letter clustering). A bounded
**coder-in-the-loop search** (A2.2: hill climbing / annealing over pair swaps,
scored by real coder bytes) is the right follow-up, with the random permutation
kept as the control. The 472 B cost is easily amortised if search finds ≳1 KB.

### A3 — information-inheritance cold starts (REJECTED)

| Method | enwik7 archive | ΔS vs parent |
|---|---|---|
| `info-inherit` | +35,333 archive | **+35,397** |
| `info-unrelated` (control) | +27,064 archive | +27,128 |

Both are worse than a neutral cold start, and — notably — inheritance from the
*true* lower-order parent is **more harmful** than the unrelated control.

**Finding (informative).** In this architecture the mixer already receives every
lower-order expert as a separate input. Seeding a child's cold slot with the
parent's *current* probability therefore injects a correlated duplicate of
information the mixer already has, reducing expert diversity and forcing the
mixer to undo the redundancy. PPMII's actual mechanism inherits *statistics*
under a different model topology (escape/backoff), not a current probability in a
mixture. A fair follow-up is a proper bit-history/state-map expert (A4/A16 /
Phase 6) rather than seeding direct tables.

### A2 + A3 interaction

`alphabet-perm+info-inherit`: ΔS **+159,923** on enwik7. Overlapping and
antagonistic; recorded for A28.

## Status vs the wave plan

| Wave | Item | Status |
|---|---|---|
| A | A0 freeze | **DONE** |
| A | A2 alphabet geometry | **DONE** — REJECTED; coder-in-the-loop search is the follow-up |
| A | A3 information inheritance | **DONE** — REJECTED; state-map/ICM is the fair follow-up |
| A | A20 optimizer sweep | **DONE** — ADOPTED (lr 24) |
| A | A17 previous-line structural expert | **DONE** — ADOPTED |
| B | A1 / A26 / A27 case-factorized FOT tokenization | A1.2 **DONE — REJECTED**; A1.1/A26 **DONE — ADOPTED** (255-token vocabulary, reverse ids); A26 v2 extension **REJECTED** (coverage past 255 loses) |
| C | A5–A11 parsing (entropy-repriced optimal parse, MRU carousel, matched-literal residuals, distance floors, ROLZ ranks) | A8 matched-literal **ADOPTED** (Phase 4.4, enwik9 −3.87 MB composite); remainder NOT RUN |
| D | A4 CTS, A16 DMC | NOT RUN — the Phase-6 spine (state maps / ICM / PPM) was tested directly and the state-map and PPM experts were REJECTED at enwik9; CTS/DMC remain unrun |
| E | A12–A15 grammar refinements | NOT RUN (Phase 5 shows grammar loses at every scale) |
| F | A18/A19 entropy throughput | NOT RUN |
| G | A23 BWT tunneling, A24 archaeology | NOT RUN |

Every executed experiment wrote a receipt to `evidence/runs/receipts.jsonl`
(candidate, parent, archive delta, measured binary cost, ΔS, decision).

The accepted-configuration stub is **324,328 B**. Relative to the frozen parent
(315,760 B) the +8,568 B is Phase-A framework scaffolding plus the adopted column
expert and the OOM guard, all charged; the enwik8 archive saving of ~135 KB
dominates it.

## Next highest-value actions

1. **A1.1/FOT follow-ups** — a case-independent id-ordering control (A32);
   coder-in-the-loop vocabulary selection (the only sound form of A27, since the
   raw-byte gain heuristic was falsified by the A26 v2 rejection); and reclaiming
   the ~21.8 KB of `std::HashMap` code (Phase-11 size).
2. **Extend `memory::projected_encode`** with an explicit transform-aux term (the
   token `HashMap`); measured enwik9 peak RSS stayed just under the projection,
   so the guard held, but the margin is thin.
3. **A2.2 coder-in-the-loop alphabet search** — cheap; the A2 control proves the
   signal exists and frequency is the wrong objective.
4. **Wave C parsing** — entropy-repriced optimal parsing + MRU carousel +
   matched-literal residuals; a genuinely different family from context mixing.

## Out-of-memory protection

A workstation that runs an editor and a compressor must not have its memory
exhausted by the compressor: the kernel's OOM killer does not distinguish
between them. Every memory-heavy path now **fails closed**:

- `src/memory.rs` computes a conservative projected peak (`projected_encode`,
  `projected_decode`) and compares it against a budget.
- Budget precedence: `--max-ram <size>` › `ZENTROPY_MAX_RAM_BYTES` ›
  `min(3/4 x MemAvailable, 8 GiB)`.
- Guards are wired into `compress`, `decompress`, `verify`, `bench`, `eval`,
  `sweep`, `hoist`, the corruption court, and the submission stub
  (`zentropy-sfx`) for compress, decompress and the self-extracting path.
- `zentropy meminfo [file]` reports available memory, the budget, and the
  projected encode/decode for a file.

```
meminfo enwik9              -> projected_encode 4.41 GiB  OK (budget 8.00 GiB)
meminfo enwik9 --max-ram 4G -> projected_encode 4.41 GiB  BLOCKED
```

The estimator is deliberately conservative: a false refusal is cheap, an OOM
kill is not.

**Measured cost.** The guard adds **4,544 B** to the scored submission stub
(measured `--no-default-features --features struct-hoist,column-model` versus
`…,mem-guard`). It is charged, kept on by default because the protection is the
point, and flagged as a Phase-11 size target.

**Build memory.** The research `release` profile now uses `thin` LTO with 4
codegen units (was `fat`/1) so rustc's own peak memory cannot OOM a workstation;
a new `research` profile (no LTO, 16 units) is available for quick iteration.
The scored `submission` profile keeps `fat`/1 for the smallest artifact.

## Standing rules

- Only a fully packaged candidate decides; coder-free entropy estimates do not.
- Every mechanism has a hostile negative control.
- Every executable cost is measured, never estimated.
- A mechanism is not rescued by layering unrelated improvements on top.
- Full enwik9 is the only final authority (A29).
