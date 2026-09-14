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

### 4.1 Accounting caveat on the in-flight gates

The T2 gates were launched with `--binary-cost 0`. That was correct for Phase 9,
whose knob rode a header byte that already existed and cost nothing, but it is **not
correct here**: `with_tune`'s scaling loop is real code, so the gate's `ΔS`
**under-counts by the executable cost** and must be read as an *upper bound* on the
gain until an otherwise-identical `accepted` vs `accepted, tune-table` submission
build is measured (A31 — measured, never estimated). The expected magnitude is tens
or hundreds of bytes against a win expected in the hundreds of kilobytes, so this
does not threaten the verdict, but it is recorded rather than glossed: a receipt
that says `ΔS` without saying which costs are in it is a half-truth. The same
caution applies to the peak-RSS claim in §4 — `peak_rss_bytes` from the receipt is
the authority, not the projection.

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

## 7. The scored stub's executable bytes — **−288,928 B measured, worth 2x in `S`**

**Why executable bytes matter twice as much as they look.** Both legal packaging
forms charge the program *twice*:

```text
separate form:   S = comp9a + decomp9 + bhm,  and with one binary comp9a = decomp9
                 S = 2 x P + bhm
self-extracting: archive9 embeds the program, so S = comp9 + archive9
                 archive9 = P + bhm + 23,  giving S = 2 x P + bhm + 23
```

So one byte of executable is **two** bytes of `S`. Measured on enwik6 with the
shipped stub: `program_bytes=111,888`, `bhm=267,333`, `archive9=379,244`,
`S(self-extracting)=491,132`, `S(separate)=491,109` — the 23-byte difference between
the forms is the SFX marker plus the length field, which is also the sanity check
that both accounting paths agree.

### 7.1 What the stub's bytes actually were

Measured composition of the 400,816 B stable stub (`objdump -h`, and
`nm --size-sort -S` on a `strip=none` build):

| section | bytes | note |
|---|---|---|
| `.text` | 295,352 | code |
| `.eh_frame` + `.eh_frame_hdr` + `.gcc_except_table` | 38,460 | unwinding tables, useless under `panic=abort` |
| `.rodata` | 27,448 | strings and the learned weights |
| `.rela.dyn` + `.dynsym`/`.dynstr`/`.dynamic`/`.got` | ~20,000 | dynamic-link machinery |

The largest *symbols* were the finding. Zentropy's biggest was `Cm::new` at 5,629 B;
above it sat `std`'s panic/backtrace apparatus —
`backtrace_rs::symbolize::gimli::Cache::with_global` (18,619 B),
`gimli::read::dwarf::Unit::new` (8,863 B), `miniz_oxide::inflate::core::decompress`
(7,344 B), `addr2line::…parse_children` (4,692 B), `rustc_demangle::try_demangle`
(2,479 B) and four `quicksort` instantiations — none of which touch the codec.

### 7.2 Four candidate levers, measured

| lever | result | verdict |
|---|---|---|
| narrow the 95-way method dispatch to the 2 ids the accepted encoder emits | **+24 B** | **REJECTED** — fat LTO already removes the config code no reachable method reaches. The hypothesis was wrong; the experiment was deleted rather than kept, since a two-method decoder is a correctness risk that buys nothing |
| `-C force-unwind-tables=no` | 0 B | no effect: the unwinding tables come from the **prebuilt** `std` rlibs, which are compiled `panic=unwind` regardless of our profile |
| `-C target-feature=+crt-static` | **+905,552 B** | REJECTED — static glibc is enormous |
| `-Z build-std=std,panic_abort` + `-Cpanic=immediate-abort` | **−288,928 B (−72%)** | **ADOPTED** |

### 7.3 The adopted lever, and why it is legitimate

`std`'s prebuilt rlibs carry a panic hook, backtrace capture, DWARF parsing
(`gimli`), symbolisation (`addr2line`), name demangling and a DEFLATE decompressor,
unconditionally. Rebuilding `std` with `panic_abort` and
`panic_immediate_abort` removes all of it.

```text
stable, --features accepted                          400,816 B
nightly-2026-07-24 + build-std + panic=immediate-abort 111,888 B
```

Verification, because a smaller binary that codes differently is worthless: on
enwik6 the nightly stub produces an archive **byte-identical** to the stable
build's (267,333 B) and reconstructs the corpus exactly; `tools/package_sfx.sh`
reports `exactness: PASS (byte-identical)` end-to-end through the self-extracting
path.

**Effect on `S`:** the program shrinks by 288,928 B, and the program is charged
twice, so `S` falls by **577,856 B** — the second-largest single reduction in the
project after the Phase-4 composite, and larger than the entire Phase-8 learned
corrector (−421,646 B).

Reproducibility is pinned in `tools/package_sfx.sh`: the toolchain is
`nightly-2026-07-24` (by date, not `+nightly`), the target is named explicitly, and
`-Z build-std` needs that toolchain's `rust-src` component. A plain stable build
still works and is 288,928 B larger — that is the documented fallback, so the source
is never hostage to a nightly.

### 7.4 `mem-guard` is the one remaining binary cost worth noting

Measured under the adopted build: `accepted` 111,888 B, `accepted` minus `mem-guard`
106,368 B → the scored startup refusal costs **5,520 B**, which is 5,520 × 2 =
**11,040 B of `S`**. It is kept deliberately: it is the OOM protection that makes a
mis-provisioned judged machine fail cleanly instead of being OOM-killed, and 11 KB
is 0.007% of `S`. The *research* half of the guard (`mem-floor`) stays outside
`accepted` and is charged nothing (see [`MEMORY_GUARD.md`](MEMORY_GUARD.md)).

### 7.5 The lesson Phase 11 keeps re-teaching

Every one of the four levers above was a plausible estimate, and three were wrong —
two of them by a factor of infinity in the wrong direction. "Most of the stub is
dispatch for rejected methods" was written in the architecture doc as near-fact and
measured at +24 B. The lever that mattered was in a place no estimate would have
looked: the standard library's panic path.
