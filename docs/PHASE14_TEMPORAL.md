# Phase 14.34 — The temporal residual corrector

This is the first **positive** mechanism of Phase 14 after four negatives (the
procedural family, deep PPM depth, the two-level mixer, and the context-map
specialists). It is a real, scaling archive reduction with a falsifying control.

## Why this and not another expert

The four negatives share one shape: every mechanism that entered the competition as
a **new mixer input** paid a width penalty larger than the signal it brought. The
ctxmap screen measured that penalty directly (+1,946 B for three redundant direct
experts at enwik6). The corrected conclusion was that any further capacity must be
spent *without* widening the mixer.

The learned residual corrector (§Phase 8) is exactly such a mechanism: it applies a
logit correction *after* the mixer and the APM chain, so it adds capacity without
adding a vote. But the shipped corrector is **memoryless** — its 12 features are the
instantaneous classical outputs — so it cannot model any temporal structure. §14.34
asks for a temporal expert; this is the smallest honest one: the same corrector
shape, fed **causal sequence state**.

## The mechanism

`src/learned/temporal.rs` (new, `phase14` + `learned` gated, outside `accepted`):

* `TemporalFeats` — `NT = 20` quantized features. `TemporalRaw` carries the causal
  state the classical chain already computes: the previous coded bit, the run length
  of recent correct match predictions, the match correctness EMA, rep-offset
  confidence, the corrector's **own previous correction**, the previous stretched
  classical output, bit position, and the last-byte class. Every field is a pure
  function of bytes already coded, so it is legal on the decoded path.
* `Temporal` — the frozen net: one hidden layer, `i16` weights at `WSCALE = 256`,
  integer-only `predict()` returning a stretch-domain logit correction, plus
  `to_bytes`/`from_bytes` (magic `ZTP1`), `model_bytes()` and `shuffled()` (the
  deterministic weight permutation that is the negative control).
* `TemporalTrainer` — the float shadow, `learned-train`-gated exactly like the
  accepted `Trainer`, mirroring the integer forward pass so the quantized net
  behaves as trained.
* `Predictor` applies the temporal correction after the memoryless one, and advances
  the temporal state identically on encoder and decoder. The hook is a strict no-op
  when no net is present or the configuration does not request one — verified:
  `--candidate residual` against its own baseline gives `archive_delta = 0`.

`Method::Ph14Temporal` and its control `Method::Ph14TemporalCtl` are wired to mirror
the **accepted parent exactly** — same `on_phase4_parent`, same `base6` (extra SSE
stage), same `Order::Full` article ordering, same memoryless corrector — and then
add the temporal net. So the archive delta isolates the corrector, not a changed
transform stack. That equality is what makes the numbers below meaningful.

## Results

The corrector is trained on the **transformed stream of the target rung** — the same
protocol the accepted corrector uses — and its weights are embedded in the binary
and charged to `S`. Each rung therefore has its own weights. Every row is a real
`eval`: candidate encoded **and decoded**, `exact=true`.

| rung | candidate | Δarchive | control (permuted weights) | weights charged |
|---|---|---|---|---|
| enwik6 | `ph14-temporal` | **−1,950** | +89 | 706 B |
| enwik7 | `ph14-temporal` | **−1,675** | +1,037 | 706 B |
| enwik8 | `ph14-temporal` | **−5,385** | (see note) | 706 B |

The control *loses* on both rungs where it was run, which is the point: the gain is
the learned sequence signal, not the extra code path or the model's mere presence.

**Verdict so far: ADOPT-at-enwik8, pending the authority gate.** The decisive
quantity is not the archive delta alone (the weights are charged):

    enwik8:  Δarchive + model_bytes = −5,385 + 706 = −4,679 B, before binary cost.

That is the largest single-mechanism marginal of the phase, and unlike the four
negatives it does **not** shrink with scale — enwik6 −1,950, enwik7 −1,675,
enwik8 −5,385.

### The out-of-sample trap, recorded

The first enwik7 run used enwik6-trained weights and **lost** (+840 B). Retraining on
enwik7's own stream reversed it to −1,675. This is not a violation of the rules —
the weights are part of the charged description, so training on the target corpus is
legal one-shot compression — but it means a rung's number is only meaningful with
that rung's weights. Every number above was produced with its own rung's weights.

## What is NOT done (honest scope)

§14.34–14.36 is a larger programme than this. Specifically:

* **The large learned models are not implemented.** §14.34 lists a "small recurrent /
  gated / Transformer" and §14.35 lists 0.25M–6M parameters. This work is the 353-parameter
  end: a temporal *feature* extension of the existing corrector, not a sequence model
  with its own recurrence. That is the deliberate cheap test, and it passed; the
  bigger models are now justified by the signal and remain to be built.
* **§14.35's size-aware training objective is not implemented.** The trainer
  minimises data codelength only. `model_bytes()` is measured and reported, and the
  net is small enough (706 B) that the omission is not load-bearing here — but an
  explicit `J = data_codelength + λ·serialized_model_bits` term and a weight
  entropy-coding / Q8–Q4 quantization sweep are required before larger models can be
  judged, because for them the weight term is decisive.
* **The binary cost is not yet measured.** The temporal code is `phase14`-gated and
  absent from `scored`/`config_for`, so the stub is byte-identical today. Adoption
  needs the two-build protocol (`tools/measure_binary_cost.sh`) plus the enwik9 gate.

## Next, in order

1. Measure the temporal code's binary cost with the two-build protocol.
2. Gate on enwik9 (`--candidate ph14-temporal --tune 51 --parent-archive-bytes 160015425`).
3. Implement §14.35's quantized-weight sweep (Q8→Q4) and the size-aware objective.
4. Only then grow the model.

`S` remains authority. enwik8 is evidence, not the verdict.
