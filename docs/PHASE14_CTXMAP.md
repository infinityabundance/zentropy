# Phase 14.30 — The context-map specialists, in the pipeline

`docs/PHASE14_FIRST_BOUNDARY.md` §6 stopped the procedural family and redirected the
phase to *better modelling of expensive running text*, because attribution puts
lexical prose at 76% of the coded codelength and the model is already ~2× below
order-0 on it. The one positive signal from that redirect was
`src/context/ctxmap.rs`: a set-associative context map with a detecting tag, a
per-slot count, run state and recency replacement, whose **count-scaled stationary
estimator beat the direct expert standalone** at orders 3, 4 and 6.

Standalone ideal codelength is a **diagnostic**, not a verdict (§0, law 1, and the
constitution's "measured, never estimated"). This subphase is the in-pipeline test
that can actually decide anything: the map is wired into the real `Predictor`
(`CtxMapSpec` / `with_ctxmap` / `with_replaced_ctxmap`, `phase14`-gated), and every
number below is the fully packaged archive produced and decoded by the shipped
coder — `exact=true` on every row.

## Design

The key is `(order-N byte-context hash) << 9 | partial-byte node`, computed once per
byte in `refresh_contexts` and read identically by `predict` and `update`. A
specialist holds `2^(bits-5) × 4 × 12` bytes when the corpus ladder gives the direct
experts `2^bits` slots: **0.75× a direct expert's memory**, the same honest
comparison the standalone screening used (the map was never given more room). Three
specialists are added, at the orders the screening selected: 3, 4 and 6.

## The candidates and their controls

Every row is `eval` on the rung named, `--tune 51`, against the receipted accepted
parent (`residual`). A negative `archive_delta` would be a win.

| rung | candidate | what it is | Δarchive |
|---|---|---|---|
| enwik6 | `ph14-ctxmap` | parent **+** 3 ctxmap specialists (3/4/6) | **+656** |
| enwik6 | `ph14-ctxmap-ctl` | parent **+** 3 *direct* order experts (3/4/6) | **+1,946** |
| enwik6 | `ph14-ctxmap-rep` | parent **−** direct 3/4/6 **+** ctxmap 3/4/6 (equal width) | **+7,283** |
| enwik7 | `ph14-ctxmap` | as above | **+70,878** |
| enwik7 | `ph14-ctxmap-ctl` | as above | **+73,721** |

All five: `exact=true`. Rejections are not blamed on roundtrip failure.

## Verdict — REJECTED

**The context-map family is rejected in-pipeline.** It does not beat the accepted
parent on either rung, and it does not beat the accepted *representation* of an
order-N context:

* **The direct experts are worth far more than the map.** Replacing orders 3/4/6 by
  map specialists at equal mixer width costs **+7,283 B** on enwik6. Whatever the
  standalone comparison measured, the shipped nonstationary `p += (target-p) >> rate`
  expert at Phase 11's adapted rates is a better order-N representation than a
  tagged, count-scaled one. This is consistent with the fact that Phase 11's
  adaptation ladder alone was worth 3,925,403 B at enwik9: the *rate* is the strong
  part of this predictor, and the stationary law trades exactly that away.

* **Appending the map cannot pay for its width.** Adding the three specialists costs
  +656 B at enwik6 and +70,878 B at enwik7 (≈3.2%).

## The one thing worth keeping

The width control isolates a real, *scaling* signal inside the negative result:

| rung | map (add) | 3 direct experts (add) | map's edge over the control |
|---|---|---|---|
| enwik6 | +656 | +1,946 | **−1,290 B** |
| enwik7 | +70,878 | +73,721 | **−2,843 B** |

The map is consistently *cheaper than three redundant direct experts*, and the edge
grows with scale (1,290 → 2,843 B). So the tagged-slot / confidence / run state does
carry incremental information the plain direct experts do not — roughly 1.3 kB per
megabyte of corpora, extrapolating cautiously. The reason it still loses is that the
only way to consume it here is to **widen the mixer**, and three more inputs cost
more than the signal they bring.

That is a statement about the interface, not the mechanism. The standing
conclusion is therefore:

> The context map's information is real but must be consumed **without widening the
> mixer** — folded into an existing input, or used to *context the* mixer/SSE rather
> than as a new member of it — or it does not exist at all.

A future attempt should condition `mix_cx` (or an SSE key) on the map's occupancy /
confidence / run state, so the extra information steers the existing competition
instead of adding a vote to it. That is a different mechanism from this one and is
not claimed here.

## What this does *not* say

It does not re-open the procedural family (already stopped), and it does not
contradict Phase 6.7's rejection of checksum-verified *direct* tables: that added an
8-bit checksum beside a 2-byte slot, whereas this is a 12-byte tagged slot, and both
were measured to lose. It says the stationary estimator is the wrong *law* for this
pipeline.

## Honesty notes

* `binary_cost` is reported as 0 in these gates and that is correct rather than a
  shortcut: the specialists are `phase14`-gated and absent from `config_for`, so a
  rejected mechanism has **no** scored-binary cost to measure. Had it won, the code
  would have had to enter the scored path and the two-build protocol would apply.
* enwik6 and enwik7 reject mechanics; enwik9 decides. The effect is ~3% and grows
  with scale in the *wrong* direction for the mechanism, so no scale reversal is
  plausible here. The gate was not run on enwik9 because a 3% loss at 10 MB is not a
  candidate a 90-minute gate could rescue.
