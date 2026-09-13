# Phase 8 — Learned residual corrector (model-size Pareto campaign)

> Deliverable (architecture §6): a learned model that consumes the **classical**
> predictor's outputs and corrects what they leave unexplained, admitted only
> through the **net-gain gate** (`PRIOR_ART_MECHANISMS.md` T8):
>
> ```text
> residual_saved  >  model_bytes + binary_bytes
> ```
>
> Prior art: the `fx2-cmix-transformer` frontier embeds a frozen, offline-trained
> ~6 M-parameter transformer whose weights are *counted in S* and which consumes
> the classical PPM model's predictions (T1, T2, T3, C14); NNCP shows the
> `-log2(p)` interface (N1–N3); N6 asks whether weights can be regenerated at
> decode instead of shipped; T9 is weight quantization.

## Method

Identical accounting discipline to Phases 4–7. A learned component is admitted
only when the complete `ΔS = Δarchive + Δbinary + model_bytes < 0` on enwik9,
with exact reconstruction and a control that destroys the learned signal while
keeping the model size and code identical.

Three structural decisions:

1. **The corrector consumes the classical chain, it does not replace it.** Its
   features are the classical mixer/APM outputs and the match/word state, so the
   learned capacity is spent on the *residual*, exactly the T2 cascade.
2. **Integer inference.** The shipped network evaluates in fixed-point integer
   arithmetic; encoder and decoder produce identical probabilities with no
   floating point on the scored path.
3. **Weights are model data and are charged.** The corrected layer's weights are
   embedded in the binary (frozen, offline-trained) and their bytes counted; the
   Pareto campaign sweeps hidden width to find the point where the residual
   saved stops paying for the model bytes.

## Ordered sequence

| # | Item | Mechanism | What is built |
|---|---|---|---|
| 8.1 | Net-gain gate harness | T8 | `residual_saved − model_bytes − binary_bytes` accounting, reported for every learned variant |
| 8.2 | Learned feature extractor | T2 | the classical feature vector (mixer/APM logits, match state/bit, bit position, word/rep state) |
| 8.3 | Frozen MLP residual corrector | T1, T2 | a 1-hidden-layer integer MLP consumed after the APM chain; offline-trained, quantized (int16), embedded |
| 8.4 | Weight quantization sweep | T9 | int8 / int12 / int16 weights: ΔS vs accuracy |
| 8.5 | Size Pareto | T8 | hidden width 0/4/8/16/32/64: residual saved vs model bytes |
| 8.6 | Online-regenerated weights | N6 | the same corrector trained online at decode (0 model bytes) as an ablation |
| 8.7 | Control | — | the same network with deterministically permuted weights (identical size/code, no signal) |
| 8.8 | Transformer assessment | N1, T3 | whether the frontier's attention architecture is reachable inside the resource envelope |

## Adoption rule

Adopted only if the complete `ΔS < 0` on enwik9, with the model bytes and the
inference code charged, exact reconstruction proven, and the permuted-weight
control failing to reproduce the gain.

## Result (measured)

### The corrector

A 1-hidden-layer integer MLP consumes a 12-dimensional classical feature vector
(the `stretch`ed mixer and APM1/APM2/APM3 outputs, the match state and predicted
bit, bit position, word/rep flags, and two disagreement terms) and emits a logit
correction after the APM chain. Weights are `i16` (scale 256) and **embedded in
the scored binary**; inference is fixed-point integer, so encoder and decoder
agree exactly with no floating point on the scored path.

The network is trained offline by online SGD on the *transformed* stream the
predictor codes (so training sees the inference distribution). Crucially, the
correction is applied only to the coded probability, not to the model update, so
the classical trajectory is independent of the network — training is therefore
ordinary SGD on a fixed dataset and cannot destabilise the classical model.

A unit test (`embedded_int_net_matches_dequantized_float`) compares the integer
runtime against a dequantized float replication of the trainer. It **caught a real
bug**: the integer forward multiplied the biases by the weight scale, so the bias
contributed 256× too much and the shipped network did not match what was trained
(mismatch 1225; archives 5–17 MB *larger* than the parent). With the scale fixed
the mismatch is ≤8 and the same weights turn into a win. This is the value of
testing the runtime against the trainer, not just the round-trip.

### Pareto over hidden width (enwik7, in-sample)

| hidden | model bytes | Δarchive |
|---|---|---|
| 4 | 120 | **−11,559** |
| 8 | 232 | −11,301 |
| 16 | 456 | −11,332 |
| 32 | 904 | −11,363 |

The curve is flat: the correction is almost entirely a low-rank function of the
classical outputs, so the smallest width is the Pareto knee. `hidden = 4` is
adopted.

### Ladder

| corpus | net trained on | Δarchive | model bytes | binary bytes | complete ΔS |
|---|---|---|---|---|---|
| enwik7 | enwik7 | −11,559 | 120 | 7,848 | −3,711 |
| enwik7 | enwik9 | −7,831 | 120 | 7,848 | +17 |
| enwik8 | enwik8 | **−81,495** | 120 | 7,848 | **−73,647** |
| enwik8 | enwik9 | −65,245 | 120 | 7,848 | −57,397 |
| enwik9 | enwik9 | *(gate running)* | 120 | 7,848 | — |

The model bytes are part of the binary (embedded with `include_bytes!`), so the
binary delta is charged once; `model_bytes` is reported separately for the
Pareto only.

### Control (8.7)

The permuted-weight control is catastrophic on cross-corpus enwik7 (**+171,749**
vs −1,056 for the real net trained on enwik8), confirming the gain is the learned
weights and not the extra correction path: identical architecture, identical
model size, identical code.

### 8.6 online-regenerated weights (N6)

Rejected for the scored path. The corrector's *online* analog already exists in
the model (the mixer and APM stages are online learners); regenerating the
network's weights at decode would require either shipping them (the adopted
route) or an integer online trainer. The current trainer is `f32`, which would
make decoder probabilities platform-dependent. Shipping the frozen quantized
weights is both cheaper in code and exactly reproducible, so N6 is closed by
construction.

### 8.8 transformer (N1/T3)

Assessed and not built. The frontier's ~6 M-parameter transformer costs ≈2.9 MB
of embedded weights plus attention at decode; the Pareto campaign finds a 120-byte
MLP with 7.8 KB of code already produces a positive net gain, and the phase's
gate is equality of accounting, not architectural ambition. A transformer is a
width/architecture point on the same Pareto curve and is only worth revisiting if
the MLP's residual saturates.

### Authority (enwik9)

| variant | archive | Δarchive | bpc | exact |
|---|---|---|---|---|
| parent `reorder-full` | 170,063,733 | — | 1.3605 | — |
| **`residual`** | **169,642,087** | **−421,646** | **1.3571** | true |
| `residual-ctl` (permuted weights) | 172,264,753 | +2,201,020 | 1.3781 | true |

**Adopted.** Passing the phase's net-gain gate (T8):

```text
residual_saved = 421,646 B
model_bytes    =     120 B   (embedded in the binary)
binary_bytes   =   7,848 B   (learned feature: code + embedded weights)
complete ΔS    = 421,646 − 7,848 = −413,798 B
```

The permuted-weight control costs **+2,201,020** with the same architecture, model
size and code, so the gain is the learned weights and nothing else.

The accepted configuration moves from 170,063,733 to **169,642,087** bytes
(1.3605 → 1.3571 bpc). Relative to the Phase-7 parent the mechanism is small but
real; relative to the start of the project it is the first time the predictor's
*residual* — rather than another deterministic model — has been learned and paid
for by measurement.
