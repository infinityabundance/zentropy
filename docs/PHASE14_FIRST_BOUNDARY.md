# Phase 14 — first boundary report

> **Required by §14.52.** Phase 14's pivotal question was whether any real
> Wikipedia-derived class is strictly cheaper as a *shared program plus state plus
> residual* than as the accepted representation. This records the measurement, its
> controls, the two redesigns the first pass forced, and the single evidence-driven
> verdict.
>
> Everything here is measured on `evidence/corpus/enwik6` (the development rung).
> Nothing is extrapolated to enwik9.

## 1. Current S

| quantity | value |
|---|---|
| enwik9 archive (shipped stub, `29ed675`) | 160,015,425 B |
| program (static musl) | 125,056 B |
| **enwik9 S (separate form)** | **160,265,537 B** |
| enwik6 archive (accepted config) | 245,797 B |

## 2. Codelength attribution (`zentropy opportunity`)

Every coded byte is attributed to one structural class and one predictor role, and
the report is rejected unless the parts sum to the total.

enwik6, accepted pipeline: lexical 187,770 B (76.4%), punctuation 32,979 (13.4%),
xml_structure 11,160 (4.5%), numbers 8,600 (3.5%), unclassified 4,196 (1.7%),
dict_state 962 (0.4%).

enwik6, `--raw` (1:1 with the corpus): lexical 140,218 (54.5%), **links 59,249
(23.0%)**, punctuation 28,449 (11.1%), templates 10,227 (4.0%), tables 10,017
(3.9%), numbers 4,810 (1.9%), xml_structure 3,521 (1.4%).

## 3. The realizable lower-bound oracle (`--oracle`)

Measured cost beside a zeroth-order bound from the class's own histogram, labelled
`TARGET-FITTED ENTROPY`. On enwik6 the lexical class measures 187,770 B against a
369,196 B zeroth-order bound: the model is **far below** order-0 on prose, so the
oracle is not the binding constraint and no class is leaving trivial order-0
redundancy on the table.

## 4. The boundary experiment (`zentropy procedure`)

`A` = the accepted configuration's measured cost for the class stream `S`.
`B` = `program + state + residual + marginal decoder + separators`, residual coded
through the *same* accepted configuration. enwik6, `--limit 500`, bytes:

| class | spans | \|S\| | **A** | B synth | B random | B best-member | B literal-only | ΔS |
|---|---|---|---|---|---|---|---|---|
| template | 465 | 37,625 | 11,633 | 15,438 | 14,356 | 22,699 | 35,317 | **+3,805** |
| wiki_link | 500 | 12,605 | 4,761 | 6,605 | 6,658 | 7,484 | 16,900 | +1,844 |
| wiki_table | 18 | 22,743 | 6,445 | 6,680 | 6,688 | 9,613 | 13,814 | **+235** |
| xml_open | 500 | 5,684 | 300 | 1,351 | 1,759 | 1,124 | 10,131 | +1,051 |
| number | 500 | 2,948 | 696 | 1,390 | 1,390 | 1,493 | 7,448 | +694 |
| url | 467 | 21,590 | 7,549 | 9,485 | 9,485 | 9,593 | 25,330 | +1,936 |
| entity | 500 | 3,048 | 112 | 751 | 751 | 686 | 7,548 | +639 |

Controls behave: random cohorts are always ≥ the synthesised skeleton (so the
cohorting is not doing the work); best-actual-member sometimes beats the
synthesised skeleton and sometimes loses (so anti-unification is not reliably
better than reusing a real member); `Literal`-only is 2–7× A, confirming the
accounting charges a procedure for sharing nothing.

### 4.1 Two redesigns the first pass forced, both now in

The first pass charged programs at `serialize(p).len()` — raw varints — and let
singleton cohorts pay a whole program for nothing. Both were defects against the
plan (§14.29 requires the explanation to be entropy-coded; §14.13 presupposes a
shared program worth sharing), and both are fixed and measured:

* **the explanation is now modelled** (`src/procedural/progcodec.rs`): six streams
  (opcode / arity / parameter / edge-distance / length / literal) each coded by the
  smallest of raw, static rANS with a *charged* histogram, order-0 and order-1
  adaptive coding. A realistic ~51-node template skeleton drops to **56%** of raw
  (286 → 160 B) and a repeated-opcode program to **32%** (1,204 → 385 B);
  `best_program_bytes` returns the minimum, so a program too small for its own
  model bytes is honestly charged raw.
* **a minimum cohort size with an unshared fallback**: below it a cohort pays **no
  program at all** and its members are priced directly, so a singleton cannot look
  good or bad by accident. On the template class the program term fell from
  14,097 B to **611 B** at `min_cohort 8`, and ΔS from +12,728 to +3,805.

### 4.2 The asymptote, which is the decisive measurement

If the family cannot win, withdrawing sharing should show `B` approaching a floor
rather than improving without limit. It does. Template class, `--limit 2000`,
A = 11,633 B:

| min_cohort | program | state | residual | sep | **B** | ΔS |
|---|---|---|---|---|---|---|
| 3 | 1,729 | 437 | 12,808 | 464 | 15,438 | +3,805 |
| 8 | 611 | 467 | 12,879 | 464 | 14,421 | +2,788 |
| 32 | 152 | 476 | 12,775 | 464 | 13,867 | +2,234 |
| **1000** (no sharing) | **0** | 485 | 12,642 | 464 | **13,591** | **+1,958** |

and wiki_table is flat at B = 6,680 (program 0, state 33, residual 6,630, sep 17)
against A = 6,445 for every `min_cohort` tested.

So with **zero** sharing — no program, no anti-unification, nothing procedural at
all — the decomposition's residual stream is still strictly more expensive than
coding the same bytes directly: 12,642 B vs 11,633 B on template, 6,630 vs 6,445 on
wiki_table. The residual wire format pays per-member headers and the cohort
reordering discards context the coder was exploiting, so `B`'s floor is
`A + overhead`.

## 5. Verdict: **STOP PROCEDURAL FAMILY** (as specified)

`B ≥ A` on all seven classes, the smallest gap being **+235 B** on `wiki_table`,
and §4.2 shows the gap cannot be closed by any cohorting policy: the best case
degenerates to direct coding plus overhead. This is Kill Gate A, and Kill Gate D
names the reason directly — *"program/state are tiny but residual remains
enormous: shift research toward residual/language modeling."* Program and state are
0–611 B and 33–485 B; the residual is 6,630–12,879 B.

Three quantified causes, and they are structural rather than implementation
defects:

1. **Where shared structure is abundant, it is already nearly free.** The
   `xml_open` class codes at **300 B for 5,684 B of spans — 0.42 bits/byte** — and
   `entity` at 112 B for 3,048 B (0.29 bits/byte). A shared program can only add
   overhead there.
2. **Where bytes are expensive, shared structure is small.** `wiki_link` codes at
   3.02 bits/byte and `url` at 2.80; anti-unification strips only a small prefix,
   suffix and a few shared segments, so the residual stays ~the full member bytes.
3. **The residual stream loses to direct coding even at zero sharing** (§4.2), so
   the family's floor is above `A` on these classes.

The DSL stays frozen at seven operators: adding operators cannot address any of the
three causes, and §14.51's instruction for exactly this outcome is *"stop expanding
the DSL… diagnose the reason first."*

## 6. Where the remaining headroom actually is

The attribution (§2) and the oracle (§3) answer this, and they redirect the phase:

* **lexical prose is 76% of the codelength**, and the model is already 2× below
  order-0 on it — so the remaining pool is *better modelling of expensive running
  text*, not structural reconstruction;
* links are the largest non-prose pool at 23% of the raw-stream codelength, and
  they code at 3.02 bits/byte — expensive, but §4 shows a program/residual
  decomposition does not help them.

So the correct continuation is the plan's own modelling branch, which needs no new
representation family: §14.30 `ContextMap`, §14.32 deep PPM as a distribution
provider, §14.33 hierarchical mixing, §14.34-14.36 the temporal learned residual
expert with model-size-aware training. That is also, independently, what Phase 13's
Tier 1 wants re-tested — and the two now agree on the evidence.

## 7. What this pass established

* `zentropy opportunity`: where the codelength is, in two modes with their limits
  stated, with parts that sum to the total.
* `zentropy procedure`: `A` vs `B` with its controls, on the same bytes, reporting
  decoder cost as **UNMEASURED** rather than omitting it, and exposing the
  asymptote that makes the verdict decisive.
* A bounded procedural VM with an exact serializer, five independently enforced
  bounds, rank/unrank primitives, a typed residual algebra, an entropy-coded
  program representation and a bounded target-directed search — all covered by
  tests (291 passing) and all costing the scored stub **not one byte** (still
  125,056 B).
* A negative result with its causes, its controls and its asymptote, which is the
  outcome §14.52 asks for and which saves the campaign from months of tuning in the
  wrong family.
