# Resource closure (Phase 11): spending the memory envelope for archive bytes

> **Status: MEASURED, NOT YET ADOPTED.** The enwik9 authority gates for the two
> candidate scales are in flight; nothing here is claimed as an adoption until a
> full `eval` on enwik9 returns `ΔS < 0` with exact reconstruction.

## 1. The finding

`ModelConfig::for_size` sizes every direct expert's table from the corpus length
and **caps the ladder at `bits = 24`**:

```rust
let bits = if n <= 2_000_000 { 18 }
           else if n <= 20_000_000 { 20 }
           else if n <= 200_000_000 { 22 }
           else { 24 };                     // <-- cap
```

Nothing in `docs/` or in the source records a measurement behind that cap; it reads
as a memory-caution heuristic (roughly four table slots per input byte). But the
Hutter envelope is **10 GB**, and the model it produces for enwik9 is **576 MB** —
under 6% of what is allowed. If archive bytes fall as the tables grow, the cap is
leaving score on the table for no reason.

They do fall. Measured on enwik8 with `layout --bits N` (all-hashed, archive bytes
are authority, timing irrelevant):

| bits | model bytes | enwik8 archive | step |
|---|---|---|---|
| 22 *(natural for enwik8)* | 213 MB | 21,245,220 | — |
| 23 | 257 MB | 21,075,411 | −169,809 |
| 24 | 425 MB | 20,950,804 | −124,607 |
| 26 | 1,431 MB | 20,813,414 | −137,390 |
| 28 | 5,458 MB | 20,777,718 | −35,696 |

Monotone over four doublings; **−431,806 B (−2.03%)** from 22 → 26. The curve
flattens (the last doubling buys 36 KB) and memory grows fast, so the interesting
window is `bits` 24–26, not larger.

## 2. Why this should transfer to enwik9 — and why that is still a guess

The mechanism is *hash capacity*: for every order whose distinct-context count
exceeds the table, the tables are oversubscribed, and doubling the table halves the
oversubscription. At `bits = 24` an order-2 expert has 16.7 M slots against 16.7 M
contexts × 255 nodes, i.e. ~256× oversubscribed, and higher orders are worse.

enwik9 is *more* oversubscribed than enwik8 at the same `bits` — same table, ten
times the data — so the direction should hold and the magnitude should be larger.
**But this project's own rule is that enwik9 is the only authority**, and it has
been burned by exactly this kind of reasoning (A1.1 wins on enwik9 and loses on
enwik7; the Phase-9 APM axis wins on the enwik7/enwik8 means and loses by 90,996 B
on enwik9). Hence the gate, not an extrapolation.

## 3. The knob: `tune-table`

The change must be **decoder-derivable**, or it is a silent-corruption bug. Two
properties make the `tune` header byte the right carrier:

* the Phase-9 APM axis was **rejected** and compiled out, so its high nibble is
  free in the scored build;
* `with_tune` is applied identically by `encode_tuned` and by `decode` (both read
  the archive's own `tune` byte), so scaling cannot desynchronise them.

```text
tune = (scale << 4) | lr_idx        # scale = extra bits on every order expert
```

`tune < 16` reproduces the pre-T2 behaviour exactly, so the accepted configuration
is unchanged. The feature is **outside `accepted`** (research plane until a gate
adopts it), and `tune-table` + `apm-tune` is a `compile_error!` — they claim the
same nibble, and letting one silently win would make a scored configuration depend
on which feature happened to be enabled.

`tune_table_scale_roundtrips` proves the property that matters: every scale
0..=3 round-trips byte-exactly, scale 0 is memory-identical to no scaling, and each
step actually grows the tables.

## 4. enwik9 geometry and eligibility

Measured `model_bytes` (the expert tables alone) for enwik9, against a **10 GB** peak
limit:

| `bits` | `model_bytes` | `tune` (scale) | notes |
|---|---|---|---|
| 24 *(natural)* | 575,684,608 | 5 (0) | accepted today |
| 25 | 911,228,928 | 21 (1) | |
| **26** | **1,582,317,568** | **37 (2)** | in flight |
| **27** | **2,924,494,848** | **53 (3)** | in flight |
| 28 | ~5.6 GB | 69 (4) | too close to the envelope to be comfortable |

Peak RSS adds the corpus (1 GB), the transformed stream (0.86 GB) and the archive
(≈170 MB), so scale 2 lands near 4 GB and scale 3 near 5.5 GB — both inside 10 GB.
The gate receipts record `peak_rss_bytes`; the claim is only as good as that number.

## 5. Interaction with the mixer learning rate (must be re-checked)

Phase 9 established that **the optimal mixer LR is a function of the predictor**:
re-tuning it after Phases 6–8 moved the optimum from LR 24 to LR 16 for −359,748 B.
A table-size change alters the predictor, so the LR optimum may move again. If a
scale is adopted, the LR ladder must be re-run against it before the configuration
is called final. The gates here hold `lr_idx = 5` (LR 16) fixed so the scale is
measured in isolation, and `docs/LAYOUT_DECISION.md` §7 records the same caution.

## 6. Relationship to the T1 layout experiment

Separate questions, same harness:

* **T1** (layout, [`LAYOUT_DECISION.md`](LAYOUT_DECISION.md)) — *where* the slots
  live. REJECTED: at best 1.29× for +66,674 B; the affordable subset is slower.
* **T2** (size, this file) — *how many* slots there are. Sells archive bytes for
  RAM, which the envelope has in abundance.

The `layout` command supports both (`--nibble` for T1, `--bits` for T2) because a
throughput change must never be able to hide behind an unmeasured ratio change.
