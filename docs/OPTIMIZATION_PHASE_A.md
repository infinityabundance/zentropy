# Optimization Phase A — Untapped Delta Campaign

> Locate and exploit compression mechanisms **not already covered** by the
> existing Zentropy architecture. This is not a feature-collection phase; it is
> an empirical search for `ΔS < 0` against a frozen parent, where `S` is the
> fully accounted Hutter score.
>
> **Make it informative, not successful.** Negative results are first-class.

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
| `word-token` (A1.1/A26, incl. composite methods) | 576 B |

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
| `column-word-token` | A1.1/A26 frequency-ranked on the accepted parent | ADOPTED at enwik8 |
| `column-word-token-reverse` | A1.1/A26 reverse ids on the accepted parent | **ADOPTED at enwik8**; enwik9 gate running |

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

### A1.1 / A26 — dynamic frequency-ranked word vocabulary (screening-positive; enwik9 pending)

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
the 255-entry id space, and the list is written as `count, (len, bytes)*`. The
transform runs after hoisting and case (`hoist -> case -> token -> perm`); its
`0x00` prefix composes exactly with the other byte transforms. Feature
`word-token`, measured marginal cost **576 B**.

Standalone (vs the `rawcm` floor) and on the accepted parent (vs `column`):

| corpus | `word-token` vs `rawcm` | `word-token-reverse` vs `rawcm` | `column-word-token` vs `column` | `column-word-token-reverse` vs `column` |
|---|---|---|---|---|
| enwik6 | +1,970 REJECTED | +1,139 REJECTED | — | — |
| enwik7 | −6,467 ADOPTED | −11,980 ADOPTED | +4,320 **REJECTED** | −4,389 ADOPTED |
| enwik8 | — | — | −70,187 ADOPTED | **−130,713 ADOPTED** |
| enwik9 | — | — | — | **running (the adoption gate)** |

`word-token-reverse` selects the same 255 words but assigns ids in *reverse*
frequency order (least frequent word gets id 1); it is the control that isolates
the value of frequency ranking from substitution itself.

**Finding 1 — frequency ranking is the wrong objective.** The reverse-order
control beats frequency-ranked ids at both enwik7 (−11,980 vs −6,467 standalone)
and enwik8 (−130,713 vs −70,187 on the accepted parent). This reproduces the A2
result in a different mechanism: for a bitwise MSB-first predictor, raw symbol
frequency is not the right objective. FOT's variable-length frequency ranking is
not what a context-mixing backend wants.

**Finding 2 — word substitution on a mature parent is a large win at enwik8.**
`column-word-token-reverse` saves **130,713 B** at enwik8, ~2.2x the column
expert's 58,737 B, at 576 B measured cost. Note the strong overlap with
existing mechanisms (A28): the same variant saves 11,980 B standalone but only
4,389 B on top of hoist + column at enwik7, i.e. much of the redundancy it
removes was already captured.

> **Gate (A29).** enwik9 is running. The A1.2 precedent is explicit: a −10,062 B
> enwik8 win became a **+427,246 B** enwik9 loss. No adoption decision is made
> before the full-corpus run completes.

> **Open caveat.** The causal story for reverse ordering is not established — it
> may be an artefact of the id byte's interaction with the CM's bit tree rather
> than a genuine ranking effect. A control assigning ids by an unrelated
> criterion (word length, say) would separate "ordering" from "substitution".
> Also note the transform's word-count `HashMap` is **not** included in the
> `memory::projected_encode` estimate; it is small relative to the model on
> enwik9 but should be added to the guard.

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
| B | A1 / A26 / A27 case-factorized FOT tokenization | A1.2 **DONE — REJECTED** (merge and mark-only both reverse at scale); A1.1/A26 dynamic vocabulary **screening-positive at enwik8, enwik9 gate running** |
| C | A5–A11 parsing (entropy-repriced optimal parse, MRU carousel, matched-literal residuals, distance floors, ROLZ ranks) | NOT RUN |
| D | A4 CTS, A16 DMC | NOT RUN |
| E | A12–A15 grammar refinements | NOT RUN |
| F | A18/A19 entropy throughput | NOT RUN |
| G | A23 BWT tunneling, A24 archaeology | NOT RUN |

Every executed experiment wrote a receipt to `evidence/runs/receipts.jsonl`
(candidate, parent, archive delta, measured binary cost, ΔS, decision).

The accepted-configuration stub is **324,328 B**. Relative to the frozen parent
(315,760 B) the +8,568 B is Phase-A framework scaffolding plus the adopted column
expert and the OOM guard, all charged; the enwik8 archive saving of ~135 KB
dominates it.

## Next highest-value actions

1. **A1.1/A26 enwik9 gate** — the full enwik9 comparison of
   `column-word-token-reverse` vs `column` is running. Do not adopt before it;
   A1.2's enwik8→enwik9 reversal is the precedent. It will also give a second
   independent enwik9 measurement of the accepted baseline.
2. **A1.1/FOT follow-ups** — a case-independent id-ordering control (A32), and
   a comparison of words vs BPE/subword vs hybrid units (A26).
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
