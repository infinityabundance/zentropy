# Throughput analysis — why Zentropy codes at 0.4 MB/s, and what is actually fixable

> **Measured.** One enwik9 pass codes an **818 MB** transformed stream in ~33 min
> at ~0.4 MB/s = **2.4 µs/byte = 305 ns/bit**. At ~3.5 GHz that is **~1,000 cycles
> per bit** for what should be a few dozen table lookups.
>
> **Diagnosis.** The cost is memory *latency*, not arithmetic and not volume
> (peak RSS is 2.5 GB against 63 GB free). And it is **self-inflicted by our index
> function**: the eight bit-level lookups of one byte land in eight unrelated
> cache lines, where the PAQ/ZPAQ lineage arranges them to land in one or two.

## 1. What a pass actually does

Per bit: ~15 direct context models, 8 state experts, match-model tiers, repeat
predictors, one logistic mixer, three APM/SSE stages, and a small learned MLP.
Per byte: 8 bits × that, i.e. **~120+ hashed table touches per byte**.

Arithmetic at 305 ns/bit is not the explanation — 30-odd lookups with a few
multiplies and shifts is tens of cycles of *work*. It is ~1,000 cycles because
most of those cycles are spent **waiting on cache misses**.

## 2. The cause, in the primary source

Matt Mahoney, *Data Compression Explained*, §4.1.2 (bitwise encoding), on the
ZPAQ direct model:

> "As a further optimization, the model is stored as a one dimensional array
> aligned on a 64 byte cache line boundary. The bytewise context is updated once
> per byte as usual, but **the extra bits are expanded in groups of 4 in a way
> that causes only two cache misses per byte**. The leading bits are expanded to
> 9 bits …, then exclusive-ORed with the bytewise context address."

and §4.1.3 (indirect models):

> "In the PAQ8 series, the hash table is designed to allow lookups with **at most
> 3 cache misses per byte**. In ZPAQ, there are **2 cache misses per byte** … The
> ZPAQ hash table maps a context on a 4 bit boundary to an array of 15 bit
> histories and an 8-bit checksum."

So the lineage this project is built on treats **cache misses per byte** as *the*
performance metric, and designs the table layout to hit **2–3 lines per byte per
model**.

### What Zentropy does instead

`src/context/mod.rs`, `ContextModel::predict`:

```rust
let h = (self.ctx ^ c0.wrapping_mul(MIX_C)) as usize;
self.idx = h & self.mask;
```

`c0` is the partial-byte bit-tree node (1..255). Multiplying it by a large odd
constant **scrambles it across all 32 bits**, so it does not select a slot near
the bytewise context — it selects a location *anywhere* in the table. The eight
nodes visited in one byte are therefore eight unrelated addresses.

| | cache lines touched per byte, per model |
|---|---|
| PAQ8 / ZPAQ (bucketed extra bits) | **2–3** |
| Zentropy (scrambled extra bits) | **up to 8**, uncorrelated |

With ~15 models that is ~120 scattered line touches per byte rather than ~30–45.
That is the 0.4 MB/s. The same scatter also defeats the hardware prefetcher,
which can do nothing with eight unrelated stream-1 accesses.

## 3. The fix, and its honest price

**Fix.** Make the table bucket-local: hash the *bytewise* context to a bucket and
let the bit-tree node pick the slot inside it —

```rust
let bucket = (self.ctx as usize) & (self.mask & !0xFF);
self.idx  = bucket | (c0 as usize & 0xFF);
```

One byte's eight lookups then touch one contiguous 512-byte region (u16 × 256):
the first is a miss, the next seven are L1 hits. Physically this is the PAQ/ZPAQ
layout, and it also makes the standard trick available: **prefetch the next
byte's bucket while the current byte codes**, because the bytewise context is
known one byte earlier.

**Price.** The 8 low bits stop carrying context, so the number of *distinct
contexts* per table falls **256×**. Holding context resolution constant would
need a 256× larger table — our model is 544 MB, so 139 GB. Unaffordable.

PAQ escapes this price by not storing a probability per (context, node) at all:
it stores a compact **bit-history** per bucket and maps history → probability
through a *shared* StateMap. That is exactly the indirect/state-map design this
project **tested in Phase 6.1 and rejected on ratio: +754,671 B at enwik9**.

So the tension is real and already measured, in one direction:

| design | cache misses per byte per model | measured ratio |
|---|---|---|
| bit-history buckets (PAQ/ICM) | 2–3 | worse here (+754,671 B at enwik9) |
| direct probability tables (**adopted**) | up to 8 | better |

**Zentropy's ratio advantage is paid for in cache misses.** That is the honest
statement, and it means "just optimise it" is not available: the fast layout is
the layout we measured and rejected.

### The experiment that is actually promising

A **hybrid**, which has not been tested: use the bucket-local direct layout only
where the 256× context loss is affordable — the **low orders** (order 0–2 have
few enough contexts that 256× fewer is still many), and keep hashed-direct
tables for the high orders where contexts are sparse and collisions already
dominate. If the low orders can be made L1-resident, the miss traffic falls
without giving up the high-order ratio that the CM actually wins on.

`tools/simd_bench.rs` already establishes the measurement discipline; this needs
a `ModelSpec` layout flag and a ladder run (speed **and** ratio, because a speed
win that costs ratio is not an adoption).

## 4. Measured wins available now (no representation change)

| lever | measured | verdict |
|---|---|---|
| `-C target-cpu=native` | **~13%** (30.0 s → 26.1 s on enwik7, same contention) | **research only** — it emits AVX2/BMI2 unconditionally and the rules say the test machines "may change without notice" |
| `-C target-cpu=x86-64-v2` | untested | candidate for the **submission**: SSE4.2/POPCNT, universally available since ~2009, no portability risk |
| `codegen-units=1` + `lto="fat"` for the research profile | untested | standard 5–15% for a hot single-threaded loop |
| software prefetch of the next bucket | not available until §3's layout lands | — |

## 5. What is *not* the bottleneck

- **Not the entropy coder.** The range coder is a rounding error next to the model (*DCE* §3.3 makes the same point for the ABC-vs-arithmetic comparison).
- **Not I/O.** Read once, write once.
- **Not the mixer.** 0.5–1% of runtime; AVX2 buys 1.10× there and 1.19–1.51× *losses* on the table path (see [`SIMD_DECISION.md`](SIMD_DECISION.md)).
- **Not memory volume.** 2.5 GB peak of 63 GB available.
- **Not the transform stages.** Reorder/tokenisation shrink 1 GB → 818 MB in seconds.

It is memory **latency per bit**, caused by a layout choice, and the fast layout
is the one already rejected on ratio. Any future claim of a large speedup should
have to say which of those two it is trading.
