# Phase 12 — Submission closure

> **Goal.** Turn the measured research artefact into a *submission*: a single,
> self-contained, rules-compliant `comp9a` + `archive9.bhm` (and the
> self-extracting `archive9` alternative) whose `S` is measured on the shipped
> binary, whose reconstruction of the canonical 10⁹ bytes is proven, and whose
> every rule-facing claim is backed by a number in `evidence/`.
>
> Phase 11 closed *resources*. Phase 12 closes *claims*.

Phase 11 ended with a scored configuration that is exact, inside the resource
envelope, and backed by receipts. What it did **not** end with is a submission:
the stub still carries dispatch for ~90 rejected methods, there is no licence
inventory, no algorithm document, no clean-build proof, and no deterministic
rebuild check. This phase produces all of them, in order, each as a measured
artefact rather than a statement.

The rules of record are [`HUTTER_RULES.md`](HUTTER_RULES.md); its §9 checklist is
the acceptance test for this phase, and every box is to be ticked with a
receipt, not with an assertion.

---

## 12.0 Freeze, and state the target

1. Freeze `HEAD`; record the revision, the toolchain (`nightly-2026-07-24`), the
   target triple, the profile and the feature set in
   `evidence/submission/BUILD.env`.
2. Recompute the dynamic prize gate from the latest accepted record:
   `gate = floor(0.99 × L)`. Maintain `T0` (accepted record), `T1` (strongest
   credible pending), `T2 = 95,000,000` (internal moonshot, not a claim).
   The gate is derived, never hardcoded.
3. State the gap honestly. As of Phase 11's close,
   `S ≈ 165,344,019 + 2 × 112,264 = 165,568,547` against a 109,685,196 gate:
   about **55.9 MB** short. No phase may present a number as competitive that is
   not.

---

## 12.1 Reclaim the stub: measured, mechanism by mechanism

The stub is **112,264 B**, and most of it is dispatch for methods that are
rejected. `--features submission` already gates the research machinery out of the
driver, and `package_sfx.sh` already builds `--no-default-features --features
accepted`, but the *method roster* itself is not yet gated: `Method::ALL` is 95
variants and every one of their `config()` arms is reachable in the stub.

Because both legal packaging forms charge the program **twice**, every byte here
is worth two bytes of `S`: reclaiming 20 KB is **40 KB of `S`**, which is a
larger effect than several compression experiments have produced.

Order of work, each step measured against the previous stub (`A31`: build both
configs with the identical pinned toolchain, twice, and take the stable delta):

1. **Gate the rejected-method roster** behind `submission`, keeping exactly the
   accepted chain plus the `reorder_parent()` downgrade path that a corpus
   failing the free-restoration precondition needs.
2. **Gate the transform pipeline** to the adopted stages. The decode side must
   keep every inverse it can be handed; the encode side need not carry variants
   it can never select.
3. **Re-measure `mem-guard`** now that its projection reads the archive header.
   The Phase-11 target from `MEMORY_GUARD.md` §5 still stands: move the check to
   the point where the config already exists.
4. **Re-check `panic = "abort"` + `overflow-checks`** in the shipped profile:
   `submission` currently enables `overflow-checks` (good — it is the court that
   catches a wrap that would abort a judged decode), and that must not be traded
   away for bytes without an explicit, recorded decision.

Each step is a separate commit with its own measurement, and the archive must be
proven **byte-identical** after each: a stub change that alters the archive is a
compression change wearing a size change's clothes.

---

## 12.2 Prove the shipped artefact on the canonical corpus

`tools/package_sfx.sh evidence/corpus/enwik9` is the authority run. It must:

- build the stub with the pinned toolchain and flags;
- compress with **that stub** (`comp9a`);
- decompress with **that stub** (`decomp9`) and `cmp` against the corpus;
- pack the self-extracting form;
- print `P`, `bhm`, `archive9`, and `S` for **both** legal forms.

Acceptance: `decode(archive9) == enwik9` byte-for-byte, both hashes recorded
(`input_sha256`, `decoded_sha256`, `archive_sha256`), and the two forms agreeing
to the known 23 bytes (SFX marker 15 + length 8) as a cross-check that the two
accounting paths are consistent.

This is the milestone the whole project exists for: **a measured `S` on the
scored corpus from the shipped binary**, not from the research driver.

---

## 12.3 Resource closure on the *shipped* geometry

Phase 11 measured the research driver. Phase 12 must measure the stub, because
that is what runs on the judge's machine.

| claim | how it is measured | limit |
|---|---|---|
| peak RAM, encode | `VmHWM` of the stub process, and its own projection printed before the run | ≤ 10 GB **total** |
| peak RAM, decode | same | ≤ 10 GB |
| peak temp disk | files the stub writes (currently: the output only) | ≤ 100 GB |
| wall clock, encode | median of repeated runs, same machine | `< 70,000/T` h |
| wall clock, decode | same | `< 70,000/T` h |
| no GPU | the binary links no GPU runtime; assert on the dynamic symbol table | required |
| no network / no external files | run under `env -i` from an empty working directory with the corpus absent | required |
| determinism | two fresh builds from a clean checkout produce byte-identical archives | required |

`T` is quoted for **all four published readings** (Intel 1-core/4-core, AMD
1-core/8-core), and the submission is judged against the *stricter* all-core
wall-clock reading, per `HUTTER_RULES.md` §3.

The stub is single-threaded, so the all-core reading costs it nothing: its wall
clock is its own. That is a deliberate advantage of not using the parallel
research path in the scored artefact.

---

## 12.4 Self-containment and the spirit of the contest

1. **Scrubbed environment.** `env -i ./archive9` from an empty directory must
   reconstruct the corpus. Any dependence on `ZENTROPY_OUT`, the working
   directory or a locale is a bug to fix or document, not a convenience to keep.
   (The output filename defaults to `data9`, which is the Hutter convention.)
2. **No hidden state.** Enumerate every file the stub opens: it must be exactly
   its own image (SFX form) or its two arguments.
3. **No corpus concealment.** State plainly, with the `grep`-able evidence, that
   the payload is entropy-coded model output and that the only corpus-derived
   data in the submitted bytes is the archive's own header, its charged
   dictionary, and the model's learned parameters — each of which is counted in
   `S`.
4. **Spirit check.** Re-read `HUTTER_RULES.md` §6 against the final artefact and
   record an explicit sign-off. The one risk worth naming: the article-layout
   permutation is derived from page ids that travel in the archive, so it costs
   0 bytes — that is a *transform*, not hidden knowledge, and the decoder
   reproduces it from submitted bytes alone. Record the argument, with the
   receipt that the identity control is exactly 0 and the restore is exact.

---

## 12.5 Licence inventory and publication

1. Produce `docs/LICENCE_INVENTORY.md`: our own MIT grant, plus every component
   in the shipped binary. The scored path links **no third-party crates** — the
   point of the `accepted` feature set — so the inventory is Rust's standard
   library (MIT/Apache-2.0 dual) plus our own code. Prove it: dump the stub's
   dependency graph (`cargo tree --no-default-features --features accepted`) and
   the dynamic symbol table (must show no `libgcc_s`-style surprises beyond what
   `build-std` links).
2. Confirm the repository's `LICENSE` is OSI-approved (MIT) and that the crates.io
   publication matches the revision being submitted.
3. Tag the submitted revision; publish the source.

---

## 12.6 The algorithm document

`docs/ALGORITHM.md`: a standalone, human-readable description of the submitted
method, suitable for a reviewer who will not read the source. It must state, in
order, what happens to the bytes:

- the pipeline (reorder → hoist → tokenize → predict → range-code) with each
  stage's inverse;
- what the archive contains and how many bytes each part costs;
- the predictor's expert roster, the mixer, the SSE and APM stages, and the
  learned corrector with its charged weight size;
- the decoder's guarantees (literal fallback, bounded allocation, no clock, no
  randomness, no environment input);
- the exact commands that reproduce the submission's `S`.

Claims in it must cite receipts in `evidence/`, and it must **not** claim
competitiveness.

---

## 12.7 Clean-build proof

From a fresh clone with no network:

1. `cargo build --offline --profile submission --no-default-features --features accepted --bin zentropy-sfx`
   must succeed. (Possible only because the scored path has no dependencies;
   `rayon` is research-plane.)
2. The `nightly-2026-07-24` pinned toolchain and `-Z build-std` step must be
   reproducible — record the toolchain's own hash and the exact `RUSTFLAGS`.
3. Rebuild twice and `cmp` the two stubs and the two archives.

If the judge builds from source, this is the difference between a submission and
a story.

---

## 12.8 The compliance checklist

Fill in `HUTTER_RULES.md` §9, every box with a pointer to the receipt that
discharges it, and add a `docs/SUBMISSION_CHECKLIST.md` that is the single page a
reviewer reads first. Anything that cannot be ticked is listed in an explicit
**open questions** section rather than quietly omitted.

---

## 12.9 Order of execution

Phase 12 is sequenced so that the largest measured win comes first and nothing
downstream has to be re-measured:

```text
12.0 freeze + targets
12.1 stub reclamation        -> re-measure every mechanism whose cost was charged
12.2 enwik9 authority run    -> the S number, both forms, exact
12.3 resource closure on the stub
12.4 self-containment + spirit sign-off
12.5 licence inventory + publication
12.6 algorithm document
12.7 clean-build proof
12.8 checklist
```

12.1 comes first because a byte of stub is worth two bytes of `S` and because
every `S` measured before it would have to be re-measured after it.

---

## 12.10 What Phase 12 must not do

- Must not present an unmeasured `S`.
- Must not trade `overflow-checks` for bytes without a recorded decision.
- Must not let the *research* plane's throughput work enter the scored path.
- Must not tick a checklist box by argument where a command could have been run.
- Must not claim competitiveness. The honest position at the start of this phase
  is ~55.9 MB short of the current gate, and that number belongs in the document.
