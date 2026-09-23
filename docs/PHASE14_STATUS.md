# Phase 14 — status, verdicts, and the adopted baseline

One page, kept current. Every row is a measurement, not a projection; the verdict
column uses the phase's own vocabulary (ADOPTED / REJECTED / INCONCLUSIVE /
PERF_ENABLER / RESEARCH_ONLY). The authority is always `S` on enwik9.

## The baseline is UNCHANGED

Phase 14 has adopted nothing. One mechanism (14.34, the temporal residual corrector)
briefly appeared to be adopted, and that adoption was **reverted**: the gate that
justified it had run a stale binary carrying the wrong weights, and once the shipped
stub was built properly it turned out to be a **regression** on the authority corpus
(+1,566,863 B of archive). The full account, including the two mistakes, is in
[`PHASE14_TEMPORAL.md`](PHASE14_TEMPORAL.md).

The current accepted point (tune 51) is therefore the pre-phase one:

| quantity | value |
|---|---|
| enwik9 archive | 160,015,425 B |
| scored stub (musl, `accepted,submission`) | 125,056 B |
| **S = 2 × program + archive** | **160,265,537 B** |
| gap to the 1 % gate (109,685,196) | 50.6 MB |

`residual` at enwik6 is 245,797 and at enwik7 2,217,223, both matching their receipts
after the revert (`Δ = 0`).

## Verdicts

| subphase | mechanism | verdict | decisive evidence |
|---|---|---|---|
| 14.1 | codelength attribution (`zentropy opportunity`) | **ADOPTED** (as tooling) | every coded byte attributed; parts must sum to total or `run()` errors |
| 14.3 | causal `SignalBus` | **ADOPTED** (as tooling) | 17 tests; research-plane only |
| 14.4–14.8 | bounded procedural VM, exact serializer, target-directed search, rank/unrank, typed residual algebra | **RESEARCH_ONLY** | built and bounded; 5 independently enforced limits |
| 14.9 | **first boundary** | **STOP the procedural family** | `B ≥ A` on all 7 Wikipedia classes; the asymptote (zero sharing) already loses (template 12,642 vs 11,633) → Kill Gate A + D |
| 14.29 | program-stream entropy coding | supporting | ~51-node skeleton → 56 % of raw; repeated-opcode → 32 % |
| 14.19 / 14.32 | deep PPM as a distribution provider — **depth** | **REJECTED** | orders 12/16/20/25 all regress vs 8 (2.6746 → 2.6844 b/B); capacity knee 16 MiB, 64 MiB regresses. Depth is not the win |
| 14.19b | selective PPM escape (min-count rule) | **REJECTED** | every threshold loses monotonically: control 2.6746 vs best 2.7156 b/B at equal memory |
| 14.30 | context-map specialists (tagged + stationary) | **REJECTED** | +656 (enwik6) / +70,878 (enwik7); the *replacement* control costs +7,283 at equal width — the direct experts are worth more |
| 14.20 | two-level (hierarchical) mixer | **REJECTED** | −3.3 % vs the flat mixer at equal input count and comparable memory |
| 14.34 | **temporal residual corrector** | **REJECTED as trained / RESEARCH** | mechanism reproduces exactly (−10,827 at enwik8 with enwik8 weights, control loses), but the enwik9 training saturated the weights (`max|w|` at the ±4.0 clamp, `sum(w2)` = +2692 vs −362) and the corrector *hurt* (+1,566,863 at enwik9). See [`PHASE14_TEMPORAL.md`](PHASE14_TEMPORAL.md) |
| 14.35 | weight-quantization sweep + size-aware objective | **NOT IMPLEMENTED — and now the prerequisite** | the enwik9 failure is a saturation/objective problem, not a model-size one |
| 14.36 | deterministic integer inference | **ADOPTED** (with 14.34) | integer-only `predict`; the int/float agreement test is derived-bound, and caught the h=8/h=16 reversal |
| 14.21–14.28, 14.31, 14.37–14.58 (and 14.34's larger models) | large learned models, MatchTrust, procedural-aware ordering, universes, recursive residual explanation, … | **NOT STARTED** | blocked on budget, not on a negative result |

## What the negatives taught, and why it redirected the phase

Four mechanisms were rejected and they share one shape: **a new mixer input pays a
width penalty larger than the signal it brings.** The ctxmap screen priced that
penalty directly (+1,946 B for three redundant direct experts at enwik6) and turned
it from a suspicion into a number.

That is why the temporal corrector is the only family in the phase that has ever
measured a win: it spends capacity behind the mixer. It has not been adopted — its
training recipe is unstable at enwik9 scale (see the 14.34 row) — but the shape of
the rule stands, and it is the reason the next attempts should also spend capacity
*behind* the mixer rather than adding votes to it.

## Next, in order

1. **Fix the temporal corrector's training objective** (§14.35): a bound or decay that
   prevents saturation, plus a check that the second layer stays near zero-sum.
   Re-test on enwik8 (10 minutes); enwik9 is 140.
2. **Then re-run the enwik9 gate with the rebuild** — the mistake that caused this
   whole detour was training and gating without rebuilding, so the rule is now
   explicit: weights are a build input, and a training run and a gate are never
   adjacent without a rebuild.
3. **The shipped stub's own full-enwik9 authority run** for the *unchanged* baseline —
   done once, and it is how the regression was caught; it should be a standing check.
4. **Mixer-LR re-descend** (Phase 9's standing rule), which nothing in this phase has
   triggered yet, since nothing was adopted.
5. §14.35's weight-quantization sweep (Q8→Q4, entropy-coded), then larger models.
6. The unstarted structural subphases (14.21–14.28, 14.31, 14.37–14.58); the
   procedural family among them is stopped by measurement, not by neglect.
