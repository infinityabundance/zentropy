# Phase 6 — Serious context-mixing spine

> Deliverable (architecture §6): **ICM/ISSE, state maps, word/stem, SSE** — the
> serious context-mixing floor. Prior-art grounding: ledger rank 2, "Classical
> context-mixing spine" (PAQ8: P1–P9; CMIX: C1/C3/C11): *"the proven
> enwik-workhorse floor; everything learned should be measured as a residual on
> top of it, not instead of it."*
>
> This is a **model-strength** phase, and the evidence of Phases 3–5 says that is
> where the bytes are: every transform-style mechanism lost, while every model
> mechanism (word/bigram experts, the previous-line/column expert, the
> long-distance/sparse match tiers, the matched-literal expert) won. The accepted
> configuration is 1.4096 bpc on enwik9; the lineage to beat runs
> lpaq1 ≈ 1.98 → paq8 ≈ 1.44 → cmix ≈ 1.17 → `fx2-cmix` ≈ 0.88.

## Method

Identical to Phases 4–5 (`PHASE4_PLAN.md`, `PHASE5_PLAN.md`): each item is
implemented exactly behind a feature flag, gets an exhaustive round-trip court, a
hostile control (or a null-input control where the mechanism only changes mixer
width), a **measured** executable cost in the accepted-reachable configuration,
a ladder, and an enwik9 gate before adoption. Nothing is skipped or deferred; a
rejection with a control is a result.

## Ordered sequence

| # | Item | Ledger | What is built | Status |
|---|---|---|---|---|
| 6.1 | Bit-history state maps | P3 | context slots store a bucketed `(n0,n1)` bit history; a shared adaptive StateMap turns state → probability (the indirect model PAQ calls ContextMap/StateMap) | **REJECTED** at enwik9 (+754,671; wins at enwik7/8) |
| 6.2 | ICM (indirect context model) | P3 | a second-order indirect model: the context is itself a learned state | **REJECTED** (control identical; gain is width) |
| 6.3 | ISSE (indirect SSE) | P3/P2 | an adaptive secondary estimator whose input is the mixer output and whose context is a state | **REJECTED** (+26,063) |
| 6.4 | SSE / APM chain expansion | P2, X4 | additional calibration stages keyed on rich contexts (order-2 hash, match state) | **ADOPTED** (`sse-3`, enwik9 −1,671,235) |
| 6.5 | Sparse context models | P6 | gapped byte contexts (e.g. skip one byte) as experts | **REJECTED** (reverses at enwik8) |
| 6.6 | Indirect (context-of-context) models | P6 | contexts over the *history of a context*, not the bytes | **REJECTED** (control-equal) |
| 6.7 | Context-slot collision control | P9, C11 | checksum-verified / nibble-bucketed slots instead of blind index aliasing | **REJECTED** (+13,779) |
| 6.8 | PPM-style byte model | C3 | a PPMd-flavoured escape model as an expert | **REJECTED** at enwik9 (+61,133; wins at enwik7/8 — fixed table capacity) |
| 6.9 | Model scaling / pruning | C1, C6 | measure ΔS against expert count and table size; delete negative-value experts (law 7) | **COMPLETE** — enwik7 suggested pruning orders 4/8/12/16, enwik8 reversed it; nothing is pruned |
| 6.10 | Word/stem context model | P5, C8 | a stem-folded word context *model* (not a transform — 4.6 showed the transform hurts) | **REJECTED** (control wins more) |

## Adoption rule

A mechanism is adopted only if the **complete** `ΔS < 0` against the accepted
parent on enwik9, with its model and code bytes charged, exact reconstruction
proven, and a control showing the gain is the mechanism and not mixer width.

## Result (measured)

### The accounting correction

The first pass of this phase rejected 6.1 and 6.4. The `state-map` and `sse-3`
methods were not listed in `hoists()` / `token_kind()` / `match2_min()` /
`sparse_tier()` / `match_byte_kind()`, so they were compared against a `phase4`
parent that had the structural hoist and the reversed word vocabulary while they
did not. `on_phase4_parent()` now defines the accepted parent in exactly one
place, and every Phase 6 method routes through it. On the corrected parent both
wins reappear on the small rungs.

### Authority (enwik9)

The small-rung picture was itself misleading — in **both** directions — which is
precisely why enwik9 is the only final authority:

| # | Mechanism | Method | enwik7 | enwik8 | **enwik9** | verdict |
|---|---|---|---|---|---|---|
| 6.4 | extra SSE/APM stage keyed on the order-2 context | `sse-3` | −14,897 | −195,864 | **−1,671,235** | **ADOPTED** (256 B) |
| 6.1 | indirect bit-history state experts | `state-map` | −32,737 | −48,250 | **+754,671** | **REJECTED (reverses at scale)** |
| 6.8 | bounded PPM-C byte model (orders 1–4 + escape/backoff) | `ppm` | −49,505 vs `sse-3` | −121,466 vs `sse-3` | **+61,133** vs `sse-3` | **REJECTED (reverses at scale)** |

`sse-3` moves the accepted archive from **176,204,762** to **174,533,527** bytes
(1.4096 → 1.3963 bpc). Its negative control, `sse-3-ctl` (the same stage keyed
on an uncorrelated distant byte instead of the order-2 context), is only
−305,341 at enwik9: the *information in the key* is worth ≈ **1.37 MB**, and the
residual gain from merely adding a third calibration stage is ≈ 0.3 MB.

Two reversals, both instructive:

* **State maps** win at enwik7 (−32,737) and enwik8 (−48,250) and lose by
  +754,671 at enwik9: the per-slot bit-history representation is competitive
  while its 2²⁴-slot tables are sparse, and becomes destructive once they
  saturate.
* **PPM-C** wins at enwik7 (−49,505) and enwik8 (−121,466) and loses by +61,133
  at enwik9. The cause is fixed capacity: order-2 owns the exact 65,536-slot
  table but orders 3–4 are hashed into 32,768 slots, so at 10⁹ bytes hundreds of
  distinct 3-/4-grams share every slot and the expert becomes structured noise
  the mixer cannot down-weight enough. Its order-1 control is far worse
  (+2,677,761), confirming the multi-order machinery is real but under-provisioned.
  A PPM with per-context capacity that scales with corpus size is a specific,
  falsifiable follow-up — not an adoption.

No amount of enwik7/8 screening would have caught either reversal. The cheap
rungs are for *direction*, never for a verdict.

### Controls (the mechanism, not mixer width)

* `sse-3-ctl` (uncorrelated distant-byte key) — enwik7 **+9,165**; enwik9 −305,341 vs the real −1,671,235.
* `ppm-ctl` (order-1 only, no multi-order backoff) — enwik7 **+11,217**; enwik9 **+2,677,761**: the gain is the escape/backoff across orders, not the extra mixer input.
* `state-map-ctl` (three direct order experts instead of state experts) — enwik7 −3,618, enwik8 −27,216: the width helps at small scale, the representation more; both reverse at enwik9 with the real state experts.

### Rejected

| # | Mechanism | Method | Evidence |
|---|---|---|---|
| 6.1 | indirect bit-history state experts | `state-map` | enwik7/8 wins, **enwik9 +754,671** — table saturation |
| 6.2 | indirect context model (learned per-context byte history) | `icm` | `icm-ctl` (permuted slot key) is **identical** (−4,294 vs −4,297 on enwik7): the learned state carries no information |
| 6.3 | indirect (state-keyed) SSE | `isse` | +26,063; control +11,018 — the indirect state misleads calibration |
| 6.5 | sparse (gapped) context expert | `sparse4g1` | −2,405 enwik7, **+5,287 enwik8** — reverses at scale |
| 6.6 | chained / deep indirect models | `indirect-chain`, `indirect-deep` | same null as 6.2 |
| 6.7 | checksum-verified context slots | `collision` | +13,779; control is archive-identical (0) — cold-starting collided slots loses more than the reduced interference gains |
| 6.9 | prune the direct orders PPM subsumes (4/8/12/16) | `spine` | −7,676 enwik7 **reverses to +28,830 at enwik8** — the high-order experts are needed at scale |
| 6.10 | stem-folded word model | `stem-model` | control (raw word) wins **more** (−4,259 vs −4,109) — the gain is the extra word expert, not the stemming |

### Byte-cost summary

Marginal executable cost in the accepted configuration (`--profile submission`,
`--bin zentropy-sfx`): state-map 480 B (rejected), **sse-3 256 B** (adopted),
ppm 624 B (rejected) — **256 B** for the adopted mechanism against ≈1.67 MB of
enwik9 saving. The executable cost is immaterial; the archive is authority.

The accepted method is `Method::Sse3` (= the accepted configuration + the extra
order-2 SSE stage). `phase6` is an alias for it. The `ppm` and `spine` methods
remain compiled behind their features so the negative results reproduce.
