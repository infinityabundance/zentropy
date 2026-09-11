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

Rejected/experimental mechanisms are **not** in the default (scored) build;
they reproduce via `--features struct-hoist,alphabet-perm,info-inherit`.

## Results

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
| A | A17 previous-line structural expert | NOT RUN |
| B | A1 / A26 / A27 case-factorized FOT tokenization | NOT RUN (next) |
| C | A5–A11 parsing (entropy-repriced optimal parse, MRU carousel, matched-literal residuals, distance floors, ROLZ ranks) | NOT RUN |
| D | A4 CTS, A16 DMC | NOT RUN |
| E | A12–A15 grammar refinements | NOT RUN |
| F | A18/A19 entropy throughput | NOT RUN |
| G | A23 BWT tunneling, A24 archaeology | NOT RUN |

Every executed experiment wrote a receipt to `evidence/runs/receipts.jsonl`
(candidate, parent, archive delta, measured binary cost, ΔS, decision).

## Next highest-value actions

1. **A1 / A26 case-factorized frequency-ordered tokenization** — the largest
   unexplored item and the one the external review ranked first. Dynamic
   corpus-derived vocabulary (possibly BPE), case as a separate low-entropy
   stream, dictionary itself compressed, all charged.
2. **A2.2 coder-in-the-loop alphabet search** — cheap; the control proves the
   signal exists and frequency is the wrong objective.
3. **A17 previous-line expert** — cheap, targets Wikipedia tables/lists.
4. **Wave C parsing** — entropy-repriced optimal parsing + MRU carousel +
   matched-literal residuals; a genuinely different family from context mixing.

## Standing rules

- Only a fully packaged candidate decides; coder-free entropy estimates do not.
- Every mechanism has a hostile negative control.
- Every executable cost is measured, never estimated.
- A mechanism is not rescued by layering unrelated improvements on top.
- Full enwik9 is the only final authority (A29).
