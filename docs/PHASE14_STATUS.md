# Phase 14 — status, verdicts, and the adopted baseline

One page, kept current. Every row is a measurement, not a projection; the verdict
column uses the phase's own vocabulary (ADOPTED / REJECTED / INCONCLUSIVE /
PERF_ENABLER / RESEARCH_ONLY). The authority is always `S` on enwik9.

## The adopted baseline moved

Phase 14.34 is the first Phase 14 mechanism to be adopted, so the project's
authority numbers are no longer the ones in older documents. The current accepted
point (tune 51) is:

| quantity | before Phase 14.34 | after adoption |
|---|---|---|
| enwik9 archive | 160,015,425 B | **159,849,941 B** |
| scored stub (musl, `accepted,submission`) | 125,056 B | **129,152 B** |
| **S = 2 × program + archive** | 160,265,537 B | **160,108,245 B** |
| gap to the 1 % gate (109,685,196) | 50.6 MB | 50.4 MB |

`ΔS = −157,292 B`. The archive row is an exact, decoded, receipted artifact
(`evidence/runs/phase14/temporal_enwik9.jsonl`); the program row is the shipped stub;
court 9 proves the two configurations are byte-identical. Docs that quote
`125,056` / `160,015,425` / `160,265,537` are describing the *pre-14.34* point.

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
| 14.34 | **temporal residual corrector** | **ADOPTED** | ΔS = **−157,292 B** at enwik9, exact; control loses on every rung it was run |
| 14.35 | weight-quantization sweep + size-aware objective | **NOT IMPLEMENTED** | the width sweep proves it matters (h=8 charged −1,184 vs h=64 +1,266 at enwik7) |
| 14.36 | deterministic integer inference | **ADOPTED** (with 14.34) | integer-only `predict`; the int/float agreement test is derived-bound, and caught the h=8/h=16 reversal |
| 14.21–14.28, 14.31, 14.37–14.58 (and 14.34's larger models) | large learned models, MatchTrust, procedural-aware ordering, universes, recursive residual explanation, … | **NOT STARTED** | blocked on budget, not on a negative result |

## What the negatives taught, and why it redirected the phase

Four mechanisms were rejected and they share one shape: **a new mixer input pays a
width penalty larger than the signal it brings.** The ctxmap screen priced that
penalty directly (+1,946 B for three redundant direct experts at enwik6) and turned
it from a suspicion into a number.

That is why the temporal corrector won where they lost. It applies a logit
correction *after* the mixer and the APM chain, so it adds capacity **without adding
a vote**. The forward rule for the rest of Phase 14 is:

> spend capacity behind the mixer, not in it.

## Next, in order

1. **The shipped stub's own full-enwik9 authority run** (the outstanding Phase-12
   receipt). It validates `S` on the actual artifact rather than on the research
   driver that court 9 proves is equivalent.
2. **Mixer-LR re-descend** (Phase 9's standing rule after any change to the
   predictor). Not expected to move — the corrector sits after the mixer with no
   feedback — but "not expected" is not "measured".
3. **§14.35**: weight-quantization sweep (Q8→Q4, entropy-coded) and the explicit
   size-aware objective — required before any larger learned model, because the
   weight bill is what killed the wider nets.
4. **§14.34's larger models** (recurrent / gated / Transformer), now justified by a
   measured signal rather than by hope.
5. The unstarted structural subphases (14.21–14.28, 14.37–14.58) remain open; the
   procedural family among them is stopped by measurement, not by neglect.
