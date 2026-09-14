# Phase 13 — The re-test campaign: every rejection was measured against a predictor that no longer exists

> **The finding this phase is built on.** Zentropy's rejected-mechanism list is
> long, and it is honest — but almost every rejection was measured against a
> **2^24-table, LR-24-then-16, rate-ladder-4/5/6** predictor. Phases 9 and 11
> then changed the model in three independent ways at once:
>
> | knobs | at the time of most rejections | now |
> |---|---|---|
> | order tables | 2^24 | **2^27** |
> | mixer learning rate | 24, then 16 | **under re-measurement** |
> | adaptation ladder | `4,4,4,5,…,6` | **`2,2,1,2,…,3,4,5`** |
>
> Two of those three are *adaptation-speed* controls, and the new measurements say
> the model wanted to adapt **much faster**: the ladder is worth −3,925,403 B and
> LR 10 beats LR 16 by a further 608,626 B. A predictor that adapts faster, over
> eight times as many table slots, has a different relationship to every mechanism
> that was rejected for supplying what the old model could not.
>
> So Phase 13 is not a hunt for new mechanisms. It is a **systematic re-test of the
> rejected list at the final geometry**, with the same controls, the same
> admission rule, and the expectation that some of these flip.

This is the highest-information-per-hour work available, and it is the correct
answer to "the model is at 1.29 bpc and cmix is at 0.88": the gap is not going to
close with hyperparameters, but the mechanisms that were *rejected by a
hyperparameter-mistuned model* have never actually been tested against the model
Zentropy now has.

---

## 13.0 Freeze the parent, and record why the re-tests are legitimate

1. Freeze the accepted configuration after Phase 12 closes, with its measured
   `S`, its receipts, and the exact geometry (`tune`, ladder, weights digest).
2. State the re-test rule explicitly in every receipt: **"previously REJECTED at
   the pre-Phase-9/11 geometry; re-measured at the final geometry."** A re-test is
   a new experiment, not a reinterpretation of an old one, and the old receipt
   must stay visible so the pair is readable as a dose–response against geometry.
3. Re-run the *positive* mechanisms' controls too. If a control no longer
   separates from its mechanism at the new geometry, the mechanism's attribution
   has silently weakened and must be re-established before anything is built on it.

---

## 13.1 Tier 1 — the context-mixing mechanisms that reversed sign at enwik9

These are first because they are the largest and the most likely to flip, and
because the *reason* they were rejected is exactly the thing Phase 11 changed.

| mechanism | old verdict | why the new geometry may change it |
|---|---|---|
| bit-history **state maps** | enwik7/8 **win**, enwik9 **+754,671** | they model *sparsity of observations*, which is precisely what 2^27 tables and a fast ladder also address — and the old verdict came from tables 8× smaller |
| **ICM / ISSE** | rejected at enwik7/enwik8 (**+26,063** on enwik7) | indirect contexts are a *second-order* adaptation; a model that now wants to adapt fast is exactly the model that may want them |
| bounded **PPM-C** expert | enwik7 −49,505, enwik8 −121,466, enwik9 **+61,133** ("fixed table capacity") | the capacity argument was a property of the *old* table budget; the new budget is 8× larger |
| **SSE stage count / keys** | one adopted (`sse-3`), an indirect one rejected | more calibration is more useful when the base prediction is sharper |
| **sparse (gapped) contexts** | adopted as a match tier, not as direct experts | re-test as direct experts at the new width |

**Method.** One mechanism per gate, full `eval` on enwik9, with the historical
control reproduced unchanged. A mechanism that wins must then be re-checked for
interaction against the others in this tier (§13.5), because they overlap by
construction.

---

## 13.2 Tier 2 — the representation mechanisms

Re-test the whole Phase-10 lexical/representation family at the final geometry,
and specifically the two that had *screening signals that grew with corpus size*
and then reversed:

- **A1.2 case factorization** — rejected "merge wins at enwik6/7 and reverses at
  enwik8; marking reverses at enwik9 (+427,246)". A reversal that large at the
  authority corpus is the strongest known candidate for a geometry effect.
- **A3 information inheritance** — rejected with a working negative control; the
  mechanism was real, the heuristic was wrong. At 2^27 tables, a *newly touched
  slot* starting at `p = 0.5` is a much more common event, so the cold-start
  problem is now larger, not smaller.
- **A2 alphabet permutation** — rejected with a coder-in-the-loop search. Cheap to
  re-run; the permutation interacts with which distinctions the bit tree asks
  first, and the experts have changed.
- **Phase 5 grammar / rank / RePair / LZBE** — rejected with the loss growing with
  corpus size. Re-test `MR-RePair` and `RLZ-RePair` at least as *encoders of the
  transform*, since a cheaper full-corpus grammar search changes the economics
  rather than the idea.

---

## 13.3 Tier 3 — the learned model, which is now the most under-used asset

Phase 8 ships a **120-byte** corrector with 4 hidden units. That is the Pareto
knee of a *different* predictor, chosen before the tables grew 8×.

1. **Re-run the width Pareto** (`hidden` 0/4/8/16/32/64/128) against the final
   geometry, with the model bytes charged. The knee has almost certainly moved:
   the corrector is the one mechanism whose capacity is trivially adjustable.
2. **Re-train against the final trajectory**, and note the measured surprise
   already in hand: weights trained on **enwik7** beat weights trained on enwik8
   itself (−62,798 vs −41,279 on enwik8). Longer training is not better here; the
   learning-rate schedule in the trainer (`lr / (1 + steps/2e6)`) decays into the
   noise floor long before a 10⁹-byte stream ends. Fix the schedule before
   spending an enwik9 gate.
3. **Only then** consider the frontier's lesson: `fx2-cmix-transformer` feeds a
   ~6 M-parameter transformer the *classical* model's predictions and spends its
   capacity on the residual. Zentropy already has the interface for that — the
   12-feature vector and the integer corrector — so the question is purely
   economic: does a larger learned corrector, with its weights charged, buy more
   archive bytes than it costs? That is a Pareto measurement, not a bet.

---

## 13.4 Tier 4 — the Zentropy-specific machinery, now that the floor is real

This is the part of the project's thesis that has not yet been tested against a
strong classical predictor:

```
inverse proceduralization
  -> configuration / rank
  -> grammar / reference
  -> context / match prediction
  -> learned residual correction
```

Phase 5 tested grammar against a 2^24 model and lost, with the loss growing with
scale. The honest reading is not "grammar does not work" but "grammar could not
pay for itself against *that* predictor". Two things must be true before it can be
re-attempted:

1. **The classical floor must be near its own limit.** Otherwise a grammar's
   saving is counted against redundancy the predictor would have removed anyway.
2. **The grammar must be priced by the actual coder** (Phase 10's lesson: three
   representation experiments were adopted on screening signals that reversed at
   the authority corpus).

So this tier runs *after* Tier 1 has been sealed, and its first experiment is a
measurement, not an implementation: on an enwik9 slice, how many bytes does the
best grammar skeleton explain that the current predictor already predicts? The
answer bounds the entire tier.

---

## 13.5 Interaction and attribution

Every Phase-13 mechanism is a re-test of something the current accepted
configuration may already cover, so **a mechanism that wins alone may be
worthless in combination**. After the individual verdicts:

1. Build the pairwise interaction matrix (`gain(A+B) − gain(A) − gain(B)`) over
   every ADOPTED mechanism, as Phase A specified and the project never completed.
2. Re-run the *mixer LR and the adaptation ladder* after any Tier-1 adoption —
   this is now the third time the predictor has moved under them, and both are
   cheap to re-measure relative to what they buy.
3. Re-optimize the article layout only after the predictor is stable again. It was
   optimized against a much weaker model, and the honest note in
   `RESOURCE_CLOSURE.md` stands: an order optimized for yesterday's predictor is
   yesterday's local optimum.

---

## 13.6 What Phase 13 must not do

- Must not treat a re-test as a licence to re-adopt. Each mechanism needs its own
  gate, its own control, and its own measured `ΔS`.
- Must not extrapolate from enwik8 to enwik9 — the project has been burned in
  *both* directions, repeatedly, and the whole reason this phase exists is that
  scale changes the answer.
- Must not skip the interaction matrix because the individual wins look
  additive.
- Must not spend an enwik9 gate on a mechanism whose *mechanics* have not been
  validated on a smaller rung first. The gate decides adoption; it does not debug.
- Must not present the re-test campaign as progress on the ratio. The gap is
  ≈55.9 MB; Phase 13 is how it gets smaller, and every step of it is measured.

---

## 13.7 Order of execution

```text
13.0  freeze + re-test rule + control re-validation
13.1  Tier 1 (state maps, ICM/ISSE, PPM-C, SSE variants)   <- largest expected
13.2  Tier 2 (case factorization, information inheritance, alphabet, grammar)
13.3  Tier 3 (learned-corrector Pareto; trainer schedule fix)
13.4  Tier 4 (proceduralization, priced against the sealed floor)
13.5  interaction matrix, LR + ladder re-bracket, layout re-optimization
```

Each tier is sealed by measurement before the next begins.
