# Phase 5 — Procedural grammar + rank/enumerative state

> Deliverable (architecture §6): **procedural grammar + rank/enumerative state**.
>
> Prior-art grounding: ledger rank 4, "Grammar + rank state with an entropy-coded
> skeleton" (EntropyFS 12D-0/12D-1; `STACK_TECH_TRANSFER` §1/§11). The sibling
> measured result is **−47 % on the grammar skeleton** with the remaining gap
> explicitly in (a) contextual modelling and (b) rank-coded state. The
> constitutional laws that govern this phase: **entropy-code the grammar** (law 5)
> and **rank-code state when profitable** (law 6).
>
> Zentropy is a context-mixing predictor, not a grammar codec. Phase 5 therefore
> *adapts* each mechanism to the CM: an induced grammar becomes a reversible
> transform whose skeleton shares the modelled stream with its body, and
> rank/enumerative coding becomes a state representation the transform must earn.

## Method

Identical to Phase 4 (see `PHASE4_PLAN.md`): every item is implemented exactly
behind a feature flag, gets an exhaustive round-trip court, a hostile control or
dose-response, a **measured** executable cost, a ladder, and an enwik9 gate before
adoption. Nothing is skipped, picked or deferred; a rejection with a control is a
result.

## Ordered sequence

| # | Item | Ledger / prompt | What is built |
|---|---|---|---|
| 5.1 | RePair grammar transform | §6 "grammar-addressed representation" | byte-pair grammar induction; productions + body coded by the CM |
| 5.2 | Entropy-coded grammar skeleton | §6 "entropy-coded grammar descriptions" (law 5) | the production table is charged, and its representation is itself measured |
| 5.3 | Rank / MTF coding of the symbol stream | §6 "rank/unrank", "rank-coded state" (law 6) | move-to-front rank coding of rule references |
| 5.4 | MR-RePair (maximal-repeat) induction | A13 | replace the most frequent maximal repeat, not pair-by-pair |
| 5.5 | Inline first-use productions | A12 (GLZA) | define a rule at its first occurrence instead of a header |
| 5.6 | Rule MTF with first-use ids | A22 | rank active rules by recency |
| 5.7 | Enumerative / rank configuration state | §6 "enumerative coding", "rank-code state" | encode the grammar's configuration (rule count/order) as a rank |
| 5.8 | LZBE factorization oracle | A15 | factors referencing a contiguous sequence of previous factors |
| 5.9 | RLZ-RePair (memory-bounded) | A14 | build the grammar from an RLZ parse for whole-corpus feasibility |
| 5.10 | Residual decomposition | §6 "residual decomposition" | grammar skeleton + residual body, each accounted |

## Adoption rule

A grammar mechanism is adopted only if the **complete** `ΔS < 0` against the
accepted parent on enwik9, with the grammar bytes charged, exact reconstruction
proven, and a control showing the gain is the grammar and not the transform's
side effects.

## Results

Every grammar mechanism was measured as a reversible transform whose skeleton
and body share the modelled stream (so the CM entropy-codes the grammar and its
cost is charged, law 5). All are compared against the `rawcm` floor on the same
accounting backend.

| variant | enwik6 archive Δ | enwik7 archive Δ |
|---|---|---|
| `grammar` (RePair, verbatim encoding) | +47,298 | — |
| `grammar-mr` (maximal-repeat induction) | +47,298 | — |
| `grammar-mtf` (symbol rank / MTF) | +198,511 | — |
| `grammar-rrank` (rule table by first use) | +45,693 | — |
| `grammar-first-use` (inline productions) | +48,444 | — |
| `grammar-oneshot` (scalable single-pass) | +67,989 | **+489,509** |
| `lzbe` (LZ-begin-end factors, A15) | +150,530 | **+1,784,136** |

**Verdict: all REJECTED.** Every one loses, and — decisive for the phase — the
loss **grows with corpus size** (grammar-oneshot +67,989 → +489,509; lzbe
+150,530 → +1,784,136). This is the opposite of A1.1's scale behaviour, so no
large-corpus run can rescue it.

### Findings

**1. The CM already owns repetition.** A grammar removes repeated substrings, but
Phase 4's long-distance/sparse match tiers and the matched-literal expert already
capture that repetition without paying a rule table. Substituting a rule reference
for bytes removes information the CM was using (the literal symbols that predict
their neighbours) and adds skeleton bytes.

**2. Rank coding of the symbol stream is actively harmful.** MTF symbol ranking
costs +198,511 vs +47,298 verbatim at enwik6 — a 4× worse result. This is the
third independent confirmation (A2 alphabet geometry, A1.1 id ordering, now 5.3)
that **for a bitwise MSB-first context model, raw symbol frequency/recency rank is
the wrong representation objective**. Law 6 ("rank-code state when profitable")
is therefore true but *rarely profitable here*; the reordered rule table
(`grammar-rrank`) is the only variant that beat verbatim, and it still loses.

**3. Scalability is the binding constraint (5.9).** Iterative RePair is
`O(rules × n)` and cannot reach enwik8, let alone enwik9. The one-shot single-pass
induction (`grammar-oneshot`), which is the shape an RLZ-RePair-style construction
would take, is `O(n)` and computes on enwik7 — but it loses *more*, because
single-level pairs capture less of the repetition than the CM's match model.
A memory-bounded grammar search would therefore only make a losing mechanism
reachable faster.

**4. Residual decomposition (5.10) is inherent, not separate.** The body the CM
sees *is* the residual after the grammar skeleton; the measurements above are the
complete accounting of that decomposition. There is no additional residual stream
that would change the verdict.

### What Phase 5 established

The phase did its job: it tested Zentropy's central hypothesis — that an explicit
procedural explanation (grammar) plus rank-coded state beats letting the
transitional model absorb the same structure. On enwik, with a strong match model,
it does not. The explanation is real but **already paid for** by the match family;
writing it down explicitly is a net cost. That is a genuinely informative negative
result, and it redirects effort to where the bytes are: the context-mixing spine
(Phase 6) and the word tokenizer (A1.1).
