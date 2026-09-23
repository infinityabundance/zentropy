# Phase 14.34 — The temporal residual corrector

**Status: research-plane. The adoption this document previously reported was based
on a mis-measurement and has been reverted. The mechanism is real — it reproduces
exactly when trained on enwik8 — but the training recipe is unstable at enwik9 scale
and produced a corrector that *hurts*. Details below, including the two mistakes, so
they are not repeated.**

## Why this mechanism, and why it fits the phase's one transferable rule

Four Phase 14 mechanisms were rejected and they share one shape: **a new mixer input
pays a width penalty larger than the signal it brings** (priced at +1,946 B for three
redundant direct experts at enwik6). The learned residual corrector applies its
correction *after* the mixer and the APM chain, so it adds capacity **without adding
a vote** — the only shape in this phase that has ever won.

But the shipped corrector is **memoryless**: its 12 features are the instantaneous
classical outputs, so it models no sequence structure. §14.34 asks for a temporal
expert. This is the smallest honest one: the same corrector shape, fed **causal
sequence state**.

## The mechanism

`src/learned/temporal.rs` (own feature `temporal`, implying `learned`; outside
`accepted`):

* `TemporalFeats` / `TemporalRaw` — `NT = 20` causal features: the previous coded
  bit, the run length of recent correct match predictions, the match-correctness EMA,
  rep-offset confidence, the corrector's **own previous correction**, the previous
  stretched classical output, the bit position and the last-byte class. Every field
  is a pure function of bytes already coded, so it is legal on the decoded path.
* `Temporal` — one hidden layer, `i16` weights at `WSCALE = 256`, integer-only
  `predict()`, serialization (magic `ZTP1`), `model_bytes()`, and `shuffled()` — the
  deterministic weight permutation that is the negative control.
* `TemporalTrainer` — the float shadow, `learned-train`-gated exactly like the
  accepted `Trainer`, mirroring the integer forward pass.
* A `Predictor` hook applying the correction after the memoryless one, advancing the
  temporal state identically on encoder and decoder. It is a strict no-op until a net
  and a configuration request it — verified: `--candidate residual` gives
  `archive_delta = 0` against its own baseline.

`Method::Ph14Temporal` and its control `Ph14TemporalCtl` are the accepted chain plus
*only* the temporal selector. That is now a test
(`temporal_candidate_is_the_accepted_chain_plus_only_the_temporal_selector`), which
pins the equivalence at n = 10⁹ in milliseconds rather than at enwik6's scale. The
test exists because the first attempt at localising the problem was to compare the
two configurations by reading them, and asking them directly was the cheaper answer.

## What is measured, and what is not

Correct (each rung's weights rebuilt into the binary before gating — see the second
mistake below):

| rung | weights trained on | Δarchive | exact |
|---|---|---|---|
| enwik6 | enwik6 | −1,950 | yes |
| enwik7 | enwik7 | −1,675 | yes |
| enwik8 | enwik8 | **−10,827** | yes |

The enwik8 number is fully reproducible: retraining and re-gating reproduces
−10,827 B exactly. Control (permuted weights, same size and code path) **loses** on
both rungs it was run (+89 at enwik6, +1,037 at enwik7), so the gain is learned
sequence signal.

NOT measured: a valid in-sample enwik9 result. See the first mistake.

## Mistake 1 — the enwik9 gate ran a stale binary

I trained the enwik9 weights and then ran the gate **without rebuilding**. Because
the weights are embedded with `include_bytes!`, the gate therefore ran the *previous*
build, carrying **enwik8-trained weights**. The `−165,484 B` it reported is not an
enwik9 in-sample result; it is enwik8 weights applied to enwik9.

The error surfaced only when the shipped stub was finally built and run on enwik9:

    stub program 129,152 B, archive 161,582,288 B
    receipted baseline: archive 160,015,425 B   -> +1,566,863 B WORSE

The shipped stub was **not** diverging from the research driver — that was checked
directly (stub and research produce byte-identical archives at enwik6, enwik7 and
enwik8; the enwik9 header records method 86, so the reorder precondition did not
downgrade it). The stub simply carried the enwik9-trained weights, and **the
enwik9-trained corrector makes the archive worse.**

Process rule from this: **the weights are a build input, so a training run and a gate
are never adjacent without a rebuild.** Repeating the gate after retraining is not
enough; the binary must be rebuilt between them.

## Mistake 2 — adopting on a mis-measured row

The adoption commit is reverted. `accepted-core` no longer contains `temporal`, and
the accepted chain in both `config` and `config_for` no longer enables the corrector.
Reverting restores the receipted baseline exactly: `residual` at enwik6 is 245,797,
`Δ = 0`.

## The actual finding — the training recipe is unstable at enwik9 scale

The two preserved artefacts, loaded through the real decoder:

| weights | max\|w1\| | max\|w2\| | entries at the ±4.0 clamp | sum(w2) |
|---|---|---|---|---|
| enwik8-h8 | 705 | 203 | 0 | **−362** (balanced) |
| enwik9-h8 | 1024 | 1024 | 2 + 2 | **+2692** (strongly positive) |

`1024 = 4.0 × WSCALE`: the enwik9 net's weights are **saturated at the trainer's
clamp**, and its second layer sums to **+2692** against the enwik8 net's −362. A
corrector whose output is systematically positive is a *biased* corrector: it pushes
every prediction toward 1, which on a roughly balanced bit stream costs far more than
it saves. That is the sign flip, and it is fully explained.

The cause is the training schedule, not the architecture: enwik9 ran 6.86 × 10⁹
steps against enwik8's 7.06 × 10⁸ (9.7×), and the only regularisation is a hard
`±4.0` clamp that the run simply saturates.

This is exactly §14.35's subject, and it is why §14.35 is the prerequisite for any
larger model here: the weight bound, a decay or size-aware term, and a schedule that
does not drive the net into its clamp. The net is small (354 B), so nothing about this
is about model bytes yet — it is about the objective.

## Honesty notes

* `S = 2 × program + archive` in this project (the shipped separate form charges the
  program twice), so a program byte costs two bytes of `S`. The withdrawn adoption
  reported `−157,292 B` on that basis; the arithmetic was right, the inputs were not.
* The temporal code is still `accepted`-free, and the stub is back to its receipted
  size. The preserved weights live in `evidence/phase14/temporal/` and are committed
  as evidence, not as a shipped model.
* The mechanism remains the only Phase 14 family with a *reproducible* archive win
  behind the mixer. It is rejected **as trained**, not as an idea.

## Next, in order

1. Fix the training objective: a bound or decay that prevents saturation, and a check
   that `sum(w2)` stays near zero. Re-test on enwik8 first — it is 10 minutes against
   enwik9's 140.
2. Only then re-run the enwik9 gate **with the rebuild**, and gate the new baseline on
   the authority rung.
3. §14.35's weight-quantization sweep (Q8→Q4, entropy-coded) and the explicit
   size-aware objective.
4. Only then grow the model.
