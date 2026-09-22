# Phase 14 — first boundary report

> **Required by §14.52.** The pivotal question of Phase 14 was whether any real
> Wikipedia-derived class is strictly cheaper as a *shared program plus state plus
> residual* than as the accepted representation. This document records the
> measurement, its controls, and exactly one evidence-driven verdict.
>
> Everything below is measured on `evidence/corpus/enwik6` (the development rung).
> Nothing here is extrapolated to enwik9, and the verdict is about the
> representation *variant tested*, not about the idea in the abstract.

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

enwik6, accepted pipeline (ideal codelength of the transformed stream):

| structural class | bytes | share |
|---|---|---|
| lexical | 187,770 | 76.4% |
| punctuation | 32,979 | 13.4% |
| xml_structure | 11,160 | 4.5% |
| numbers | 8,600 | 3.5% |
| unclassified | 4,196 | 1.7% |
| dict_state | 962 | 0.4% |

enwik6, `--raw` (1:1 with the corpus, full ZIR taxonomy):

| class | bytes | share |
|---|---|---|
| lexical | 140,218 | 54.5% |
| **links** | **59,249** | **23.0%** |
| punctuation | 28,449 | 11.1% |
| templates | 10,227 | 4.0% |
| tables | 10,017 | 3.9% |
| numbers | 4,810 | 1.9% |
| xml_structure | 3,521 | 1.4% |

Links are the largest single structural pool after prose, which is why
`wiki_link` is one of the classes the boundary experiment tests.

## 3. The realizable lower-bound oracle (`--oracle`)

Per class, the measured cost is shown beside a zeroth-order bound from that
class's own byte histogram, labelled `TARGET-FITTED ENTROPY`. On enwik6 the
lexical class measures 187,770 B against a 369,196 B zeroth-order bound — the
model is *far below* zeroth-order on prose, which is the expected direction and
means the oracle is not the binding constraint. No class showed a measured cost
above its own zeroth-order bound, so there is no degenerate class where the
current model is leaving trivial order-0 redundancy on the table.

## 4. The boundary experiment (`zentropy procedure`)

`A` = the accepted configuration's measured cost for the class stream `S`.
`B` = `serialize(program) + state + residual + marginal decoder + separators`, with
`residual` coded through the *same* accepted configuration.

enwik6, `--limit 400`, all figures in bytes:

| class | spans | \|S\| | **A** | B synth | B random | B best-member | B literal-only | ΔS synth |
|---|---|---|---|---|---|---|---|---|
| template | 400 | 35,622 | 10,995 | 22,260 | 22,416 | 21,532 | 39,298 | **+11,265** |
| wiki_link | 400 | 10,146 | 4,023 | 5,778 | 5,778 | 5,505 | 13,750 | +1,755 |
| wiki_table | 18 | 22,743 | 6,445 | 6,745 | 6,745 | 7,937 | 22,923 | **+300** |
| xml_open | 400 | 4,631 | 296 | 1,642 | 2,339 | 1,078 | 8,232 | +1,346 |
| number | 400 | 2,425 | 593 | 1,197 | 1,197 | 1,263 | 6,025 | +604 |
| url | 400 | 18,696 | 6,633 | 8,622 | 8,622 | 8,441 | 22,301 | +1,989 |
| entity | 400 | 2,445 | 94 | 674 | 674 | 562 | 6,045 | +580 |

A single-class run at `--class template --limit 2000` (465 spans, \|S\| = 37,625 B):
A = 11,633; B synth = 24,361 — **program 14,097 + state 346 + residual 9,454 +
decoder 0 (UNMEASURED) + separators 464** — vs random 26,037, best-member 23,077,
literal-only 41,887. ΔS **+12,728**.

### 4.1 Controls

* **Random cohorts at equal size distribution**: always ≥ the synthesised
  skeleton, so the cohorting is not doing the work.
* **Best-actual-member as the shared base**: sometimes beats the synthesised
  skeleton (xml_open, wiki_link, url, entity) and sometimes loses (number,
  wiki_table, template). Anti-unification is therefore not reliably better than
  reusing a real member.
* **`Literal`-only, no sharing**: 2–7× A, confirming the accounting is sound — a
  procedure that shares nothing is correctly charged for sharing nothing.

## 5. Verdict: **REDESIGN**

**No class wins.** `B ≥ A` for all seven classes; the smallest gap is **+300 B**
on `wiki_table`, against +12,728 B on `template`. By Kill Gate A the representation
*variant tested* is **rejected**: a shared byte-level skeleton plus a `Patch`
residual does not beat coding the same bytes directly.

Three mechanical causes are visible in the numbers, and all three are fixable, so
this is a redesign rather than a stop:

1. **The program was charged uncompressed.** On `template` the program is 14,097 B
   of a 24,361 B B-cost — **58%** — and it is charged at
   `serialize(program).len()`, i.e. raw varints. §14.29 is explicit that the
   explanation is itself data and must be compressed (opcode / arity / edge /
   length / parameter streams under their own structural model). This pass did not
   do that, so the largest single term in B is unmodelled. On this evidence the
   family has not yet been tested as specified.
2. **Cohorting is too weak.** 465 template spans collapse into cohorts dominated
   by singletons, and a singleton cohort degenerates to a `Literal` program — the
   worst of both worlds. §14.13 requires the program to be paid once and shared,
   which presupposes a minimum useful cohort size and an explicit non-shared
   fallback that does not pay a program at all.
3. **The residual loses to direct coding.** On `wiki_table` the residual is
   6,649 B where A is 6,445 B: re-coding the mismatch *through the accepted coder*
   is worse than coding the original bytes, because the original bytes are
   structured text that the models already exploit while a patch stream is not.
   This is the sharpest finding of the pass: a `Patch` decomposition only pays if
   the base is close enough that the residual is a *small* stream, and here it is
   not.

Per §14.51's instruction — *"If that cannot be demonstrated: stop expanding the
DSL. Diagnose the reason first"* — the DSL is frozen at seven operators and no new
operators are added until cause 1 is fixed and the experiment re-run.

## 6. Estimated remaining headroom

The honest position: this experiment does **not** bound the family's headroom,
because the largest term in `B` was unmodelled. What it does bound is the
*uncompressed-skeleton* variant, which is dead on every class.

The next measurement that would actually bound the family is the same experiment
with the program entropy-coded under its own structural model, plus a minimum
cohort size. If `B` still exceeds `A` on every class with the program compressed,
then the residual-vs-direct finding (cause 3) is structural and the family should
be **stopped**; if `B` drops below `A` on any class, that class becomes the first
Phase 14 adoption candidate and the campaign continues.

## 7. What this pass did establish

* `zentropy opportunity` measures where the codelength is, with parts that sum to
  the total, in two modes with their limitations stated.
* `zentropy procedure` measures `A` against `B` with its controls, on the same
  bytes, and reports decoder cost as **UNMEASURED** rather than omitting it.
* The bounded procedural VM, its exact serializer, five independently enforced
  bounds, the rank/unrank primitives and the typed residual algebra all exist and
  are covered by tests (265 passing) without costing the scored stub a byte.
* The negative result is recorded with its causes rather than hidden, which is the
  outcome §14.51 asks for.
