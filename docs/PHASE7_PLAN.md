# Phase 7 — Article-layout compiler

> Deliverable (architecture §6): **semantic / structural / residual / predictor
> orders** for the Wikipedia article sequence. Prior-art grounding: ledger rank 1
> (`PRIOR_ART_MECHANISMS.md`: C12, S1–S5, W-family): *"the highest proven ratio per
> unit risk: offline search is free, decode is negligible, and it changes what
> every downstream model sees."*

## The insight that makes it cheap

enwik9 is a concatenation of `<page>` blocks. Two independent measurements on the
canonical corpus (`tools/p7_probe.sh`):

1. every `<page>` start is the literal `"  <page>\n"` and every end is
   `"  </page>\n"` — there are **0** unindented occurrences, so the split cannot
   be confused with article text;
2. the **page id** (the first `<id>` in the block) is **strictly ascending**
   across all 243,425 complete pages (0 violations).

Because the id travels *inside* the block, the original order can be restored by a
**stable sort on the embedded page id** — the permutation itself costs **zero
archive bytes**. The encoder is free to spend unbounded offline search on an order
that compresses better; the decoder is only a sort. This is stronger than the
prior-art "sort titles" restoration (titles are *not* sorted here: 106,526
violations), and it removes the explicit-permutation cost entirely.

If the ascending-id precondition fails, the encoder falls back to the parent
method and the decoder never sorts, so the transform can never corrupt a corpus.

## Method

Identical to Phases 4–6: each ordering is a method behind the `reorder` feature,
gets an exhaustive round-trip court, a control (`reorder-id` runs the same
split/permute/reassemble machinery with the identity order, isolating the
machinery cost from the ordering effect), a ladder, and an enwik9 gate. The
decoder is shared by every ordering and is a page-id sort, so the *only* thing
that can differ in `ΔS` is the compressibility of the reordered stream.

## Ordered sequence

| # | Item | What is built |
|---|---|---|
| 7.1 | Reversible page splitter + free restoration | header/pages/tail split on `"  <page>\n"`/`"  </page>\n"`, ascending-id precondition with parent fallback, sort-based restore; `reorder-id` control |
| 7.2 | Title order | bytewise title sort (alphabetical grouping) |
| 7.3 | Size order | page byte-length grouping |
| 7.4 | Structural order | namespace group (Template/Category/Wikipedia/Image/...) then title |
| 7.5 | MinHash/LSH semantic order | word-shingle MinHash signature; lexicographic signature sort places similar pages adjacent |
| 7.6 | Greedy predictor order | nearest-neighbour walk minimising a content-similarity proxy (research plane, bounded) |
| 7.7 | Permutation-cost court | explicit permutation encoding cost, to quantify what the free id-sort saves |
| 7.8 | Interaction + gate | best ordering on the accepted configuration, enwik9 authority |

## Adoption rule

A transform is adopted only if the **complete** `ΔS < 0` against the accepted
parent on enwik9, with the decoder-sort code bytes charged, exact reconstruction
proven, and the identity control showing the gain is the ordering and not the
machinery.

## Result (measured)

### The free permutation (7.7)

`zentropy reorder-info evidence/corpus/enwik9`:

```
pages=243425
page_id_strictly_ascending=true
explicit_permutation_bytes=500557
permutation_bytes_paid_by_id_sort=0
```

The id-sort restores the original order exactly and costs **0** bytes where an
explicit permutation would cost 500,557. Measured executable cost of the whole
`reorder` feature (splitter + page-id parser + restore + the encoder orderings):
**9,392 B** in the submission stub — immaterial against the archive effects
below.

### Ladder (vs the accepted parent `sse-3`)

| Order | method | enwik7 | enwik8 |
|---|---|---|---|
| control: identity | `reorder-id` | **0** | — |
| title | `reorder-title` | −11,841 | −159,489 |
| structural (namespace, title) | `reorder-struct` | −11,837 | −159,792 |
| first-template | `reorder-template-key` | −8,384 | — |
| boilerplate-signature (MinHash) | `reorder-template` | −1,144 | — |
| first-category | `reorder-category` | −16,566 | −278,299 |
| all-categories (category set) | `reorder-category-set` | −20,306 | −311,046 |
| **category set + template set** | `reorder-full` | **−21,722** | **−317,313** |
| size | `reorder-size` | +1,702 | — |
| word-MinHash | `reorder-minhash` | +6,211 | — |
| greedy nearest-neighbour | `reorder-greedy` | +6,153 | — |
| **negative control: shuffle** | `reorder-shuffle` | **+25,519** | — |

The `reorder-id` control returns *exactly* 0: the split/permute/reassemble
machinery is byte-neutral, so the entire effect is the ordering. The shuffle
control destroys locality while keeping the page set and machinery identical and
costs **+25,519**: the gain really is adjacency, not the rearrangement itself.

Findings that refine the prior-art intuition:

* **Markup-structural order wins; semantic-similarity order loses.** Grouping
  pages by the categories (and templates) they use — i.e. by the machinery they
  share — is the strongest order by a wide margin. Word-MinHash and greedy
  nearest-neighbour, the fx2/starlit "put similar articles together" idea,
  *hurt* (+6.2 KB). What the predictor exploits is shared boilerplate, not
  semantic proximity: pages in the same category carry the same infoboxes,
  navboxes and section scaffolding, so the match model and the context tables
  see near-identical local structure.
* **The effect grows with corpus size**: category-set gives −20.3 KB at enwik7
  and −311.0 KB at enwik8. `reorder-full` (category set, then template set, then
  title) is best on every rung: −21.7 KB / −317.3 KB.

### Authority (enwik9)

| Order | method | enwik9 archive | ΔS (archive) | bpc |
|---|---|---|---|---|
| parent | `sse-3` | 174,533,527 | — | 1.3963 |
| first-category | `reorder-category` | 170,544,241 | −3,989,286 | 1.3644 |
| category set | `reorder-category-set` | 170,172,208 | −4,361,319 | 1.3614 |
| **category set + template set** | `reorder-full` | **170,063,733** | **−4,469,794** | **1.3605** |

All three reconstruct byte-for-byte (`exact=true`). The ladder ordering is
preserved exactly from enwik7/8 to enwik9 — a rare case in this project, and a
sign the mechanism is genuine rather than a small-corpus artifact.

**Adopted: `reorder-full`.** Measured executable cost of the `reorder` feature
(the decoder's split/page-id/restore plus the encoder's orderings) is
**24,208 B**, so the complete `ΔS` is **−4,445,586 B**. The accepted
configuration moves from 174,533,527 to **170,063,733** bytes (1.3963 → 1.3605
bpc).

Net of the phase: the article-layout compiler is worth ≈4.45 MB of Hutter score
at a cost of 24 KB of executable and zero bytes of stored permutation — the
largest single-mechanism win in the project so far, and the first Phase that
changes what every downstream model sees.
