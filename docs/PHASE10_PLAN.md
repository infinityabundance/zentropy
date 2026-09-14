# Phase 10 — Equivalence-preserving representation optimizer

> Deliverable (architecture §6): make the choice of how a span of the corpus is
> *represented* a **measured, cost-driven decision** rather than a fixed pipeline
> stage — without ever risking exact reconstruction.
>
> This is the phase where Zentropy stops being `prediction → range coding` with a
> hand-written pretransform, and becomes the loop it was designed to be:
>
> ```text
> inverse proceduralization → representation choice → context/match prediction
> ```
>
> Status: **PLAN**.

## 0. The rule that shapes the phase

Two constitutional laws bind harder here than anywhere else, because a
representation optimizer is exactly the kind of mechanism that can silently corrupt
a corpus:

1. **Exactness.** Every representation must invert to the original bytes *from the
   archive alone*. A span representation is admissible only if the decoder can
   determine, from information it already has, both the span's extent and which
   representation was chosen.
2. **Complete-cost accounting.** Every choice the encoder makes costs bytes. A
   representation opcode, a length, a parameter, a dictionary entry, a restoration
   side stream, and the executable that implements it are all part of `S`.

That pair is the whole difficulty. It is easy to invent a cleverer representation;
it is hard to make one that is free to decode and still cheaper after its own
description is charged. The phase is therefore structured as a search for
`ΔS < 0`, not as a feature list.

## 1. What already exists (and therefore what Phase 10 is *not*)

The pipeline already applies four fixed transforms, each adopted by measurement and
each with a decode-side inverse:

| stage | encoder | decoder inverse | cost |
|---|---|---|---|
| structural hoisting | fixed 31-entry XML tag table → 1-byte codes | table in `.rodata` | measured 1,728 B binary |
| word tokenization | corpus-derived word vocabulary, reverse-frequency ids | dictionary stored in the archive | measured 21,848 B binary + dictionary bytes |
| article layout | `<page>` blocks ordered by category/template/title | stable sort on the embedded ascending page id | 24,208 B binary, **zero** permutation bytes |
| column / match / SSE / residual | context geometry, not representation | identical on both sides | measured per mechanism |

Phase 10 does **not** re-run those, and it does **not** re-open the mechanisms
already closed by measurement: grammar/RePair/LZBE (Phase 5, all rejected), case
factorization (A1.2, rejected in full), reprised alphabet permutation (A2) and
information inheritance (A3) (Wave A, rejected with controls), state maps / ICM /
ISSE / PPM (Phase 6, rejected), repeat-offset state, distance floors, stemming,
phrase and affix dictionaries, front-coding (Phase 4, rejected).

What none of those did is what Phase 10 adds: **per-span choice under a cost
oracle calibrated by the real coder.**

## 2. The representation families that compete

For a span of the transformed stream, the encoder may emit any of:

```text
Literal            raw bytes
Token              a whole-word vocabulary id (exists today)
Subword            a BPE/subword sequence                        (A26)
Affix              prefix/suffix composition of a known token     (A1.3)
Delta              previous-token rank / recency index
Match              an LZ reference (exists today, in the predictor)
Rule               an in-line first-use production                (A12)
```

Candidate generation is *cheap and greedy*; **selection is where the measurement
lives**. The families are deliberately the ones this project has already tested in
isolation, so the phase's contribution is the *superposition*, not the components.

## 3. Where the choice lives — the exactness design

Three possible carriers, in increasing cost and decreasing fragility:

| carrier | decoder derives it from | opcode cost | admissible when |
|---|---|---|---|
| **A. derivable** | the already-decoded prefix, by the same rule | 0 bits | the rule is a deterministic function of context alone |
| **B. in-line opcode** | an escape byte in the stream, read before the span | entropy of the opcode alphabet | the encoder's choice is *not* recoverable from context |
| **C. side stream** | a separate opcode stream, framed in the header | its own coded bytes | opcodes correlate poorly with the main stream |

The phase must **prefer A, measure B, and treat C as a fallback**: A is free and is
the reason the article-layout compiler is the project's best mechanism; C is what
makes a "smarter encoder" expensive.

The design that makes B affordable is **first-use definition** (A12.1): the first
time a span uses a representation, the stream carries its definition *and* its use;
later uses carry only a reference. A rule that is never used never exists, so the
grammar header is not paid for speculatively. This also composes with the existing
tokenizer: its dictionary is already a first-use structure.

## 4. The cost oracle (A5: iterative entropy repricing)

A greedy choice on heuristic costs is a guess. The phase therefore prices
representations with the coder's own measurements:

```text
pass 0:  heuristic/prior prices  ->  parse  ->  emit opcode+token streams
pass 1:  measure the actual per-symbol costs from pass 0's streams
         re-price every candidate edge
         backward optimal parse (bounded candidate graph)
         re-emit
repeat:  bounded at 3–5 passes, or stop when the measured archive stops shrinking
```

Non-negotiables:

* **Authority is archive bytes.** An entropy estimate may guide the parser; only a
  real encode of the real archive decides. Each pass ends with
  `encode_specs_layout`-style measurement, not with a Shannon estimate.
* **Backward DP, not greedy repair.** The parse must be reproduced exactly from the
  retained predecessor information.
* **Bounded.** A parse that does not terminate is a bug, not a research result.

## 5. What is charged (and therefore what can kill the phase)

```text
ΔS(representation family F) =
      Δ payload bytes (the opcode+token stream, entropy-coded)
    + Δ dictionary/definition bytes (charged, compressed, first-use)
    + Δ restoration side stream (case marks, length escapes, ordering)
    + Δ payload of every *other* stream F perturbs
    + Δ executable bytes                              (measured, never estimated)
```

The last two are what kill plausible ideas. Raising the tokenizer's coverage changes
the stream the CM sees, which changes its collisions and its mixer behaviour; a
family that wins on its own stream can lose on the composite. Every family is
therefore measured **against the accepted composite**, never against a bare model.

## 6. Staged experiments

Each is its own `Method` variant (or a `--features` gate on the existing one), its
own receipt, and its own `eval`. No stage is adopted on a smaller rung.

| # | stage | family | screen | why it is here |
|---|---|---|---|---|
| 10.1 | oracle harness | — | enwik6/7 | `opcode-tokens` measurement path: candidate generation, real-coder repricing, and a **no-op representation set** as the negative control (must produce `ΔS = 0` exactly) |
| 10.2 | subword superposition | Subword | enwik6/7/8 | A26: words vs BPE vs hybrid, one cost oracle, dictionary charged |
| 10.3 | affix composition | Affix | enwik6/7/8 | A1.3: rare inflections as prefix/suffix of a known token |
| 10.4 | token recency | Delta | enwik6/7/8 | raw id vs frequency rank vs MTF rank, entropy-coded |
| 10.5 | in-line productions | Rule | enwik7/8 | A12: profitable rules only, defined at first use |
| 10.6 | optimal parse | — | enwik7/8 | 10.1–10.5 under the repricing parser instead of greedy selection |
| 10.7 | composite + interactions | — | enwik8 → enwik9 | A28: pairwise interaction matrix over the adopted set; only the composite is gated |

Promotion path is unchanged: microfixture → enwik6 → enwik7 → enwik8 → enwik9.
The accepted configuration is tuned for enwik9 and *regresses slightly* on the
smaller rungs by design, so small-rung results are screening instruments only.

## 7. Negative controls (every stage)

| stage | control |
|---|---|
| 10.1 oracle | identity representation set — must be exactly 0 |
| 10.2 subword | subword ids permuted, dictionary intact |
| 10.3 affix | affix decomposition with a deliberately unrelated stem |
| 10.4 recency | ranks replaced by raw ids (the current scheme) |
| 10.5 productions | equal-frequency shuffled substrings as "rules" |
| 10.6 parse | random *valid* edge costs, same graph |
| 10.7 composite | shuffle control per stage, and pairwise interaction measured, not assumed |

A mechanism whose claimed signal survives its control is not yet measured; it is
merely plausible. The project has already been saved twice by this rule (the A17
column expert's `ColumnNoLine` null control, and the T1 layout's 0.97× "speedup").

## 8. Court and gate requirements

* **Exactness court:** every `Method` variant decodes byte-identically on the
  microfixture and on a hostile corpus (malformed UTF-8, NUL bytes, truncated
  pages, a corpus with no words at all, an all-same-byte corpus). The whole
  `tune` space round-trips per variant, as `tuning_variants_roundtrip` does today.
* **Deletion law:** a stage whose marginal `ΔS ≥ 0` is removed and recorded as
  REJECTED with its control; a stage that wins but makes the composite worse is
  recorded as ANTAGONISTIC, which is a result, not a failure.
* **Binary cost:** measured by `tools/measure_binary_cost.sh`-style otherwise-identical
  builds, never estimated (the structural hoist's first estimate was 543 B against a
  measured 1,728 B).

## 9. Adoption rule

A stage is adopted only when a **full `eval` on enwik9** returns `ΔS < 0` **with
exact reconstruction**, where `ΔS` includes payload, dictionary, side streams,
perturbed streams, and measured executable bytes. Nothing weaker earns a place.

## 10. Results

### 10.4 Token recency (move-to-front / move-to-second ids) — **REJECTED**

`IdMode::{Static, Mtf, MoveToSecond}` behind the `id-order` feature, exposed as
`residual-mtf` and `residual-move-to-second`. The id list is maintained by both
sides from the id sequence alone, so the transform stores **zero** side-stream bytes
and stays invertible from the archive. Measured by full `eval` (exact on every rung):

| corpus | parent | `residual-mtf` | Δ | `residual-move-to-second` | Δ |
|---|---|---|---|---|---|
| enwik6 | 267,333 | 284,261 | **+16,928** (+6.3%) | 284,128 | **+16,795** |
| enwik7 | 2,370,164 | 2,583,787 | **+213,623** (+9.0%) | 2,583,728 | **+213,564** |

Both rejected, and the loss **grows with scale**, which identifies the mechanism
rather than leaving it as a mystery: a rank transform destroys the *absolute byte
identity* of a token. A context-mixing predictor with a match model and high-order
contexts exploits exactly that identity — the same word is the same byte string in
every context it appears in — and recency ranking makes the same word a different
id depending on history. The measureable prediction was seconds versus kilobytes;
the project's repeated warning that rank transforms must be ablated per stream is
now backed by a number.

No executable cost is charged because the decision is already negative on archive
size alone; adopting it could only add bytes.

**The general lesson carries forward.** The v2 coverage result
([`OPTIMIZATION_PHASE_A.md`](OPTIMIZATION_PHASE_A.md) A26, `+776,196` at enwik8) and
this one point the same way: the predictor exploits *stable, sparse* identities, so
a representation change pays only when it makes the model's job **easier**, not when
it makes the byte string shorter. That is the bar the remaining stages are judged
against.

### 10.1b Model-priced vocabulary — **REJECTED**, and the screening had the wrong sign

**The hypothesis.** The shipped membership rule keeps a word when
`count * (len - 2) > len + 1`, i.e. it prices a token at 2 *raw* bytes and a literal
at `len` raw bytes. But the predictor does not code raw bytes. Since the A26 result
showed that *adding* coverage past 255 costs 776,196 B, the natural reading was that
substitution is being priced wrong — so reprice it against what the model actually
charges.

**The instrument** (`vocab-price`, research-only). The range coder accumulates its
own ideal code length, and one untokened pass attributes cost to each word run, so
every candidate word gets a measured `literal_bits`, a token price and a definition
price at the stream's measured average bits/byte. The vocabulary is a *stored
dictionary* read back by the decoder, so choosing it is purely encoder-side: no
format change, no decoder change, no exactness risk.

**The screening looked strong.**

| corpus | shipped vocabulary gain | repriced top-255 gain | screening headroom | overlap | shipped entries with gain ≤ 0 |
|---|---|---|---|---|---|
| enwik6 | 87,527 bits | 135,458 bits | **+47,931 bits (5.9 KB)** | 155/255 | 26 |
| enwik7 | 842,839 bits | 1,198,019 bits | **+355,179 bits (43.4 KB)** | 178/255 | 23 |

The worst offender was `the`: 6,459 occurrences, measured gain **−9,171 bits** — the
model predicts it almost for free, so substituting it should *cost* bytes. `nbsp`,
`xml`, `space`, `preserve`, `www`, `REDIRECT` were negative too; `is`, `by`, `or`
(short words the `len >= 3` filter excludes) appeared in the repriced top ten.
Headroom was ~1.8–2.2% of the archive and *grew* with scale, which looked like the
largest Phase-10 opportunity by an order of magnitude.

**The authority said no.** Full `eval`, exact on both rungs:

| policy | enwik6 | Δ | enwik7 | Δ |
|---|---|---|---|---|
| `residual-priced` (re-rank by measured gain) | 269,498 | **+2,165** | 2,396,034 | **+25,870** |
| `residual-price-filter` (keep the shipped vocabulary and **ids**, stop substituting the negative-gain words) | 270,087 | **+2,754** | 2,403,665 | **+33,501** |

Both REJECTED, and the *filter* — the policy aimed at the screening's most confident
signal — is the **worse** of the two.

**Why the screening was wrong, stated precisely.** A counterfactual cannot be priced
inside a model that does not have the mechanism. `literal_bits` was measured in a
pass where *no* word was tokenised, so the contexts, the match model's history and
the mixer's state around each word were all different from the model that would
actually do the substitution. The measured cost of `the`-as-literal in that model is
not the cost of `the`-as-literal in a model already saturated with tokens. The error
is not a scale factor: it changed the **sign**.

**Consequences.**

1. The shipped count heuristic is *near-optimal for this predictor* despite being
derived from a clearly wrong model of what coding costs. "The pricing is wrong" was
a sound criticism that measurement did not turn into bytes.
2. The A26 coverage rejection was about **dilution**, not pricing. Repricing does not
rescue it.
3. A Shannon-cost screen is not a weak-but-directionally-useful oracle here; it can
be confidently backwards. `S` is authority, and this is that law demonstrated on a
screen built specifically to try to avoid needing it.

131 tests pass with `vocab-price`, 126 by default.
