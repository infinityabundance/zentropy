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

## 8. Results — T2 is **ADOPTED**, and the LR ladder is closed

### 8.1 Table size (T2): measured wins at enwik9

All gates used `tune < 16` as the parent (scale 0, the Phase-9 accepted
configuration) and reconstructed exactly. `tune = (scale << 4) | lr_idx`, so
`tune 37` is scale 2 and `tune 53` is scale 3, both at LR 16.

| tune | scale | order tables | archive | Δ vs Phase-9 accepted (169,282,339) | exact | peak RSS | wall |
|---|---|---|---|---|---|---|---|
| 5 | 0 | 2^24 | 169,282,339 | — | true | ~3.5 GB | ~54 min |
| 37 | 2 | 2^26 | 166,328,552 | **−2,953,787** | true | 4.36 GB | 55 min |
| 53 | 3 | 2^27 | **165,344,019** | **−3,938,320** | true | 5.63 GB | 58 min |

The cap in `for_size` was leaving **3.9 MB** on the table, and it was there for no
recorded reason. Scale 3 is well inside the envelope: 5.63 GB measured peak
against the 10 GB rule, and a conservative projection of 6.53 GiB for encode /
5.59 GiB for decode against the 8 GiB the stub will approve.

**Adoption is complete, not pending.** `tune-table` is in the `accepted` feature
list and `ACCEPTED_TUNE = 53`, so the scored stub encodes *and* decodes the
scale-3 geometry, deriving the scale from the archive's own header byte. Three
things were required first and are all measured:

| step | result |
|---|---|
| LR re-bracket at the new geometry (§8.2) | LR 16 remains an interior optimum |
| marginal executable cost (A31) | **192 B** per copy, stable across rebuilds → **384 B of `S`**; net **ΔS = −3,937,936** |
| the scale must be bounded (`MAX_TABLE_SCALE`) | a `tune` byte from a forged header cannot order an unbounded model; see §8.4 |

The binary cost is measured by `tools/measure_tune_table_cost.sh`, which builds
`accepted` against `accepted-core,pre-t2-geometry` — the same mechanism set with
the nibble inert — using the *same* pinned toolchain, target and `build-std`
flags as the packaging step, because at `opt-level="z"` artifact size is a layout
property and comparing across toolchains would measure the toolchain.

### 8.2 Mixer learning rate: the ladder is closed at LR 16 — now at scale 3 too

The Phase-9 ladder had only been probed *downward* from LR 24 (24 → 20 → 16, each
step buying more, which is why the next rung was gated rather than extrapolated).

The five Phase-9 gates have run, against the Phase-9 accepted LR 16:

| tune | mixer LR | archive | Δ vs LR 16 |
|---|---|---|---|
| 5 | **16** | **169,282,339** | — (Phase-9 adopted) |
| 0 | 12 | 169,449,876 | **+167,537** |
| 4 | 10 | 169,757,310 | +474,971 |
| 3 | 8 | 170,416,502 | +1,134,163 |
| 2 | 6 | 171,623,140 | +2,340,801 |
| 1 | 4 | 174,028,421 | +4,746,082 |

Every step below 16 regresses, monotonically. Combined with LR 20 (+158,058) and
LR 24 (+359,748) above it, **LR 16 was an interior optimum at scale 0**.

Phase 9's own lesson is that the optimum is a function of the predictor, and a
table-size change alters the predictor, so the neighbours were re-gated **at
scale 3** rather than assumed. Both are worse:

| tune | scale | mixer LR | archive | Δ vs tune 53 (165,344,019) |
|---|---|---|---|---|
| 52 | 3 | 10 | 165,761,543 | **+417,524** |
| 53 | 3 | **16** | **165,344,019** | — (adopted) |
| 54 | 3 | 20 | 165,533,121 | **+189,102** |

**LR 16 is an interior optimum at scale 3 as well**, bracketed on both sides, so
the ladder is closed rather than merely stopped.

> **Reading the 52/54 receipts.** Those two runs were killed by an external
> process (the user's session terminated them) at 77% and 82% of the *decode*
> pass, so they are recorded as **size measurements with an interrupted
> exactness decode**, not as complete gates. Their verdicts are nonetheless
> sound: the archive *format* is identical to tune 53's, whose full `eval` gate
> decodes exactly, and the whole 0..=255 `tune` space round-trips in the unit
> court, so a different mixer learning rate cannot change decodability. Both are
> rejected on size, and a rejection needs no exactness proof. The `tune 53`
> receipt is a complete gate.

### 8.3 Binary cost and the final geometry

Shipped stub: `accepted`, `--profile submission`, nightly `build-std` with
`panic_immediate_abort`. The `mem-guard` correction in §8.4 (projecting the
archive's declared geometry) and the new `tune-table` scaling code together move
the stub from **111,888 B → 112,264 B**. Of that, **192 B** is the scale
mechanism and the rest is the guard becoming correct rather than cheap. Both
legal packaging forms charge the program twice, so the scale's cost in `S` is
**384 B** against a **3,938,320 B** archive win.

### 8.4 A decoder hole that the test-plane guard exposed

Adopting T2 introduced two defects that had nothing to do with compression:

1. **An attacker-chosen allocation.** The scale lives in the high nibble of
   `tune`, and `tune` is read from the archive header. Unclamped, a forged byte
   could ask `decode` for 2^(base+15)-slot tables — precisely the "allocates
   without bound" failure the corruption court exists to forbid.
2. **A guard that cleared it.** `mem-guard` projected
   `ACCEPTED_METHOD.config_for(n)`, i.e. the configuration *we* would have
   chosen, not the one the archive declares. A forged header was therefore
   checked against the wrong model and passed.

Both are fixed structurally. `context::MAX_TABLE_SCALE = 3` clamps the scale at
the largest value the project runs (scale 3, the adopted point), so **the
accepted geometry is the worst case for any archive**; and `archive::peek_header`
plus `memory::projected_decode_for` make both the stub's and the driver's startup
guard project the configuration the header actually names. The corruption court
now asserts the bound for **all 256** `tune` values and decodes an adversarial
`tune` rather than only mutating bytes at random.

Scale 4 is therefore not *inconclusive* any more, it is **excluded**: doubling
the order tables again projects ~9.7 GB, past the 8 GiB the stub will approve and
within 0.3 GB of the hard 10 GB rule. Recording that as an eligibility boundary
is more useful than an untested question.

### 8.5 T2's dose–response, and what it says about the mechanism

The gain grows with corpus size, which is what a collision-reduction mechanism
should do — the larger the corpus, the more contexts are competing for slots:

| corpus | Phase-9 accepted (tune 5) | scale 3 (tune 53) | Δ |
|---|---|---|---|
| enwik6 | 267,333 | 262,750 | −4,583 |
| enwik7 | 2,370,164 | 2,333,062 | −37,102 |
| enwik8 | 21,245,220 | 20,865,077 | −380,143 |
| enwik9 | 169,282,339 | **165,344,019** | **−3,938,320** |

Every rung reconstructs exactly (`evidence/runs/ladder_t2/`). Note the
superlinear jump from enwik8 to enwik9 (10× the corpus, 10.4× the saving): at
enwik8 the base `for_size` size is 4× smaller, so scale 3 is comparing different
absolute geometries. That is also why the smaller rungs are screening
instruments and never a licence to extrapolate.

## 9. Adaptation rates — another unjustified constant, worth several MB

### 9.1 The finding

Each direct expert's adaptation shift (`p += (target - p) >> rate`) is its memory.
The shipped ladder uses 4 for the low orders and 5–6 for the high ones. That
*sounds* right — sparse contexts should move less — but there is no recorded
measurement behind it, and it is arguably backwards: a high-order context is seen
rarely, so it needs to become confident from few observations.

`zentropy rate-sweep` screens it. Unlike the vocabulary screen in
[`PHASE10_PLAN.md`](PHASE10_PLAN.md) 10.1b, this one is **trustworthy**, because
every point is a real encode of the real archive by the real coder — there is no
counterfactual to get wrong. It is still encode-only, so a winner is baked in as a
constant and re-gated on enwik9.

A uniform shift already showed the direction: `scope=all`, delta −2, is **−68,594 B**
on enwik7, while +1 and +2 are +45,146 and +90,426. A per-expert coordinate pass then
put **every** expert on a faster rate except `MatchByte` (already fine), with `Word`
alone worth −42,747:

```text
rates  4,4,4,5,5,5,5,6,6,6,5,5,5,5      (shipped)
   ->  2,2,1,2,2,2,3,3,3,3,2,3,3,5      (best per expert)
```

Sum of individual gains −136,512, combined −69,115: the optima overlap strongly, so
the gains are not additive.

### 9.2 The trend matters more than the number

The scale-0 vector was found on enwik7. Carrying it up the ladder:

| corpus | baseline | with the enwik7 vector | Δ | Δ as a fraction |
|---|---|---|---|---|
| enwik6 | 267,333 | 255,990 | −11,343 | −4.2% |
| enwik7 | 2,370,164 | 2,301,049 | **−69,115** | −2.9% |
| enwik8 | 21,245,220 | 20,833,501 | **−411,719** | −1.9% |

The fraction *shrinks* as the corpus grows, the same way the mixer learning rate's
optimum moved (Phase 9). So the enwik7 vector is a **lower bound on the direction,
not the answer**: the enwik9 optimum is probably slower than enwik7's, and only an
enwik9 gate can settle it. A naive linear extrapolation of −1.9% would claim ~−3 MB;
this project has been burned by exactly that reasoning, so no number is claimed here.

### 9.2b The screen redone at the adopted geometry — and a harness bug it found

T2 changed the geometry, and a rate screen is only meaningful against the geometry
it will ship with, so the screen was rerun at **scale 3** (`--tune 53`). That
immediately failed its own sanity check: `rate-sweep --tune 53` reported a baseline
of `2,370,164`, which is the *scale-0* number. The cause was real and worth
recording — `encode_specs_layout` installs the caller's expert roster verbatim, and
`cmd_rate_sweep` was building that roster from `method.config_for(n)` **without**
`with_tune`, so the screen silently measured the unscaled model no matter what
`--tune` said.

Fixed two ways: the roster is now built through `with_tune`, and the command
**self-checks** — it encodes the baseline through both the roster path and the real
coder and refuses to report deltas if they disagree. That check is one extra
encode and it is the reason this class of bug cannot come back silently.

With the harness honest, the picture at scale 3 (enwik7):

| screen | Δ vs the shipped ladder |
|---|---|
| uniform, `scope=all`, −2 | −96,033 |
| uniform, `scope=all`, −3 | −93,747 (−1: −60,233; +1: +75,260; +2: +152,446) |
| per-expert coordinate, `scope=all` | **−106,709** (sum of individual gains −199,033) |

The direction is unchanged (faster is better) and the magnitude is **larger than at
scale 0** (−106,709 vs −69,115) — the expected shape, since larger tables mean
sparser contexts, which is exactly the regime where adapting fast pays. Uniform
`scope=all` is a blunt instrument (it moves a low order and a high order together),
so the coordinate pass gives the vector that was baked:

```text
shipped  4,4,4,5,5,5,5,6,6,6,5,5,5,5
baked    2,2,1,2,2,2,2,3,3,3,2,3,4,5      (context::ACCEPTED_RATES)
```

Every expert wants a faster rate except `MatchByte`, which is already at its
optimum. The vector is one named constant applied once, in `Method::config`, rather
than 14 scattered literals — so the ladder cannot drift from what was screened.

Baking it reproduces the screen exactly on enwik7 (2,226,353, −106,709) and gives
enwik6 `246,808` (−15,942 vs the T2 point).

Its binary cost was measured rather than assumed, and the measurement was worth
running: the shipped stub moves **112,264 B → 112,392 B (+128 B, stable across two
identical builds)**, because the `with_rates` ladder reaches the binary even though
only the *values* changed. Both packaging forms charge the program twice, so the
ladder's price in `S` is **256 B**. That is noise beside a multi-megabyte archive
win, but "it is only a constant" is exactly the reasoning the constitution exists
to refuse.

The enwik9 verdict is a full `eval` gate in flight; nothing is claimed about it
until it lands.

### 9.3 Status, and the coupling to be careful about

**NOT YET ADOPTED.** The rates were screened and the vector is **baked** into
`context::ACCEPTED_RATES`, but adoption is decided by a full `eval` gate on enwik9
against the T2 parent (165,344,019). Everything about the sequence is deliberate:

1. adopt the T2 scale (already gated: −3,938,320 exact at scale 3), then
2. re-screen the rates **at scale 3**, bake them, and gate the pair on enwik9, then
3. re-check the mixer LR at the final geometry (Phase 9's rule: the LR optimum is a
   function of the predictor).

Step 3 is not optional. Steps 1 and 2 both changed the predictor, and the LR ladder
was last closed against the predictor that existed *before* either of them. The
`52`/`54` bracket in §8.2 closes the LR ladder at scale 3 with the **old** rates; if
the rate change is adopted it must be re-bracketed once more, and that gate is the
next thing after the rate verdict.

Coupling is real in both directions, which is why each step is its own gate rather
than a joint search: a combined point would make `ΔS` unattributable.

### 9.4 Scale 4: from INCONCLUSIVE to **excluded**

The scale-4 gate (`tune 69`, 2^28 order tables) died with
`memory allocation of 428851336 bytes failed` under the 9 GiB `RLIMIT_AS` I set. That
is a limit I imposed, not a property of the mechanism, so the run itself decided
nothing.

But the question it was asking has since been answered by arithmetic instead of by
a re-run, and the answer is that scale 4 is **ineligible**: doubling the order
tables again projects ~9.7 GB of model plus buffers, which is past the 8 GiB the
scored stub will approve and within 0.3 GB of the hard 10 GB rule. A configuration
whose peak has no margin against a hard requirement is not a candidate, however
well it compresses. `context::MAX_TABLE_SCALE = 3` makes that boundary structural
— see §8.4 — so the scale-4 question is closed as an eligibility limit rather
than left open as an untested one.

## 10. Phase 12: reclaiming the stub, measured byte by byte

The stub is charged **twice** by both legal packaging forms (`S = 2P + bhm`, and
`archive9` embeds a copy), so every executable byte is worth two bytes of `S`.
This section records the reductions, each measured by building two
otherwise-identical stubs and taking a stable delta (A31).

### 10.1 The offline trainer was linked into the scored stub

**Found by accident, and it was worse than a size bug.** The scored stub's
dynamic symbol table contained `log2f@GLIBC_2.27` — a *libm* call. Tracing it:
`learned::Trainer` (the floating-point shadow used only to produce
`weights.bin`) was gated on `learned`, which *is* in `accepted`. The stub never
constructs a trainer, so the code was unreachable at runtime — but it was linked,
which means the judged binary depended on the host's floating-point library for
nothing at all.

That is a determinism question the project had been answering by argument ("the
integer path never reads a float") when it could be answered by a symbol table.
The trainer now lives behind `learned-train` (research only: in `default`, not in
`accepted`), and the guarantee is one command:

```sh
nm -D --undefined-only <stub> | grep -iE 'log|exp|pow|sqrt|round'   # must be empty
```

| build | stub bytes | libm | `NEEDED` |
|---|---|---|---|
| `accepted` + `learned-train` (before) | 112,392 | `log2f` | `libm.so.6`, `libc.so.6` |
| **`accepted` (shipped)** | **109,432** | **none** | `libc.so.6` |

**−2,960 B per copy = −5,920 B of `S`**, and the archive is proven unchanged:
byte-identical on enwik6 (246,808 B) and enwik7 (2,226,353 B) between the two
stubs. A size change that alters the archive is a compression change wearing a
size change's clothes, so that check is not optional.

The general lesson: **a feature gate is a claim about what is in the binary, and
only the symbol table can confirm it.** `#[cfg(feature = "learned")]` reads as
"the learned corrector", but the field it gated was the *trainer* — one letter of
intent away from shipping the wrong thing.

### 10.2 Still open in Phase 12

`Method::ALL` is 95 variants and every one of their `config()` arms is reachable
from the stub's `method_from_id` dispatch, so a large share of the remaining
109,432 B is dispatch for methods that are rejected. Gating that roster is the
next measured step; see [`PHASE12_PLAN.md`](PHASE12_PLAN.md) §12.1.
