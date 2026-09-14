# Table-layout decision (T1): the bucket-local layout is REJECTED

> **Question.** [`THROUGHPUT_ANALYSIS.md`](THROUGHPUT_ANALYSIS.md) §3 identified one
> untested fix for the ~0.4 MB/s coding rate: make a model's table *bucket-local*
> (`idx = bucket | node`) so a byte's eight bit-level lookups land in one or two
> cache lines, instead of the up-to-eight unrelated lines the hashed index
> produces. It predicted the low orders could afford the 256× context loss.
>
> **Answer, measured.** The loss is 16×, not 256×, and it is still too expensive:
> the mechanism's best case is **1.29× for +66,674 B** of archive, and the
> affordable subset is **0.97× (slower) for +7,071 B**. Not an adoption.

## 1. Method

`zentropy layout <in> [--nibble <orders>] [--bits <n>] [--reps <n>]` measures a
table-layout or table-size change **on both axes at once**, in one process:

* **archive bytes** (authority — `S`),
* **median wall time** over `--reps` A/B-alternated arms (A B A B …), so drift,
  thermal state and cache warmth apply equally to both arms.

The control arm is the current scored configuration, re-encoded in the same
process, and the reported rate is per *coded stream* byte, not per input byte.

Two sanity checks that the harness is measuring the real thing: at the natural
table size the control reproduces the receipted archive exactly (enwik6
`267,333`; enwik7 `2,370,164`).

## 2. Is it memory latency at all? — yes, measured

`--bits` overrides every direct expert's table size while leaving the number of
table probes *identical*. Same corpus, same model structure, same coded stream
(`8,837,054` bytes); only the working set changes:

| forced bits | model bytes | ns/byte | archive |
|---|---|---|---|
| 14 | 39,141,376 | 1293.2 | 2,530,075 |
| 16 | 40,124,416 | 1448.8 | 2,477,044 |
| 18 | 44,056,576 | 1629.7 | 2,418,046 |
| 20 *(natural for enwik7)* | 59,785,216 | 1951.3 | **2,370,164** |
| 22 | 122,699,776 | 2434.6 | 2,341,202 |
| 24 | 374,358,016 | 2499.8 | 2,328,598 |

Throughput falls **1.93×** (1293 → 2500 ns/byte) as the working set grows 9.6×,
with no change in arithmetic or probe count. So the diagnosis in
`THROUGHPUT_ANALYSIS.md` §2 is correct in kind: this is a working-set problem.

Two consequences that matter more than the headline:

* **enwik9 is already on the flat part of the curve.** It runs at `bits = 24`
  (374 MB), where the last 251 MB of working set cost only 2.7% (2434.6 → 2499.8).
  There is no cheap working-set win left by *shrinking* anything.
* **The curve is the ceiling for locality tricks.** A layout that eliminated
  *all* per-byte line scatter could at most recover the 1293-ns regime, i.e. ≈1.9×,
  and only if it cost no ratio.

## 3. Why the bucket-local layout cannot deliver that

The doc's 256× figure came from `idx = bucket | (c0 & 0xFF)`, which gives up all
eight low bits. The implementation here is the **nibble** form: the low four index
bits are the intra-nibble node, and the bucket is derived from the bytewise context
plus the nibble's prefix (`c0 >> 4` *is* the relative node, and the high nibble is
`c0 & 15` exactly at the high→low transition). That costs 16×, not 256×, and
`nibble_layout_keeps_a_byte_in_two_buckets` proves the property it buys: over
**all 256 byte paths** and four contexts, one byte touches at most two buckets,
while the hashed control scatters.

16× is still fatal for every order that matters, because affordability requires

```text
2^(bits - 4)  >=  17 * (number of distinct contexts)      # 1 + 16 buckets per context
```

At `bits = 24` (the enwik9 size) that is `1,048,576 >= 17 * #ctx`, so
**`#ctx <= 61,681`**:

| order | distinct contexts | bucket-local affordable at bits=24? |
|---|---|---|
| 0 | 256 | yes (trivially) |
| 1 | 65,536 | **no** — 1.1 M buckets needed vs 1.05 M available |
| 2 | 16,777,216 | no, by 284× |
| 3–16 | ≥ 16,777,216 | no, and increasingly hopeless |

Only order 0/1 come close, and their tables (2 MB and 32 MB) are exactly the ones
that were *already* the most cache-friendly. The big, miss-generating tables are
the ones that cannot be laid out this way.

## 4. The measurements

enwik7, `--bits 24` (emulating enwik9's cache regime with the same table sizes and
probe count):

| configuration | archive | Δ archive | ns/byte | speedup |
|---|---|---|---|---|
| control, all hashed | 2,328,598 | — | 2499.9 | — |
| `--nibble 0,1` (the affordable set) | 2,335,669 | **+7,071** | 2577.0 | **0.97×** |
| `--nibble` all 10 order experts (ceiling) | 2,395,272 | **+66,674** | 2001.0 | **1.29×** |

The affordable set is *slower* — the per-byte branch and bucket derivation cost
more than they save, because only one of the ten order tables (order 1) actually
gains. The ceiling, where every order pays the 16× (and much worse), reaches only
1.29×.

**Verdict: REJECTED.** `ΔS` is `+7,071` B and `+66,674` B respectively. `S` is
authority; a speedup is not a compression improvement.

## 5. The budget-transfer court (A19/A38) — why 1.29× would not help anyway

Throughput only earns its keep if it enables a stronger model that produces a
smaller `S` inside the time limit. It cannot here, because the limit is not close:

```text
enwik9 encode + decode      ~1.5 core-hours
Hutter allowance (AMD)      ~53 core-hours        -> ~35x headroom
```

So even a *free* 3× would change nothing about `S`. The honest role of throughput
work in this project is research velocity, not score — which is why it is recorded
as `PERF_ENABLER` at best and never as an adoption.

## 6. What actually bought research velocity (all already adopted)

| lever | measured | cost |
|---|---|---|
| rayon across candidate tunes | **3.0×** (4 concurrent, byte-identical archives) | research-only; rayon outside `accepted` |
| `--parent-archive-bytes` in `eval` | halves a gate (2 passes, not 4) | 0 |
| running N gates concurrently | ~1.4 MB/s aggregate vs 0.28 solo | RAM only |
| `progress` feature | makes a 45-minute pass observable | 21.6 KB, outside `accepted` |
| `-C target-cpu=native` | ~13% | research-only, not portable |

## 7. Limitations, stated rather than hidden

* The ceiling measurement is on enwik7 with forced `bits = 24`. That reproduces
  enwik9's *table sizes* and probe counts but not its context distribution, so the
  real enwik9 speedup may differ. It cannot differ enough to matter: the archive
  cost (+66,674 B on an 8.8 MB stream) scales with the corpus, and `S` already
  rejects it.
* One design variant was **not** tried: placing a byte's two nibble buckets
  *adjacent* so they share a cache line. The implementation deliberately scatters
  them (`k * MIX_C`) so that different contexts do not contend for one region. If
  this mechanism is ever revisited, that is the first ablation to run — with the
  same two-axis harness.
* The `--bits` sweep shows table size is a live ratio lever (bigger tables win
  archive bytes until memory binds). That is a *separate* question from layout and
  is not addressed here; it belongs to resource closure (Phase 11), and the
  measured curve above is the input to it.
