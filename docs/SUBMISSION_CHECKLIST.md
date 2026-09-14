# Submission checklist

> The single page a reviewer reads first. Every row is either a **measurement**
> with the command that produced it, or an explicit **OPEN** item. Nothing here
> is ticked by argument where a command could have been run.
>
> Status at revision `4caa5de`. The rules are transcribed in
> [`HUTTER_RULES.md`](HUTTER_RULES.md); its §9 is the acceptance test.

## A. Correctness

| # | requirement | status | evidence |
|---|---|---|---|
| A1 | `decode(archive9) == enwik9` byte-for-byte, verified by two independent hashes **and** a byte comparison | **PENDING the authority run** | `tools/package_sfx.sh evidence/corpus/enwik9` performs exactly this and refuses to continue otherwise. Last full-corpus proof: the Phase-11 gate at tune 53 (`evidence/runs/t2_gate/tune53.jsonl`, `exact=true`, `decoded_sha256 == input_sha256`). **The shipped 105,536 B stub has not yet been run on the full 10⁹ bytes** — smaller rungs are proven (enwik6/7 byte-identical to the research driver). |
| A2 | `S` computed with the exact counting rule for the chosen form | **DONE (enwik6/7)** | `tools/package_sfx.sh`; enwik6: `S_sfx = 457,903`, `S_separate = 457,880`, differing by exactly 23 B. `src/score/mod.rs` has the three forms as unit-tested constructors. |
| A3 | The two legal forms agree on the program's real size | **DONE** | The 23-byte cross-check above. |
| A4 | Every transform has a universal literal fallback; malformed input cannot break the decoder | **DONE** | `corrupt-court` (379 mutations, no panic, no unbounded allocation); `negative-court`; IR round-trip on random and malformed input. |
| A5 | No panic path can abort a judged decode | **DONE** | `panic = "abort"` plus `overflow-checks = true` in the scored profile, exercised by `cargo test --profile submission` (137 tests). |

## B. Resources

| # | requirement | status | evidence |
|---|---|---|---|
| B1 | peak RAM ≤ 10 GB, encode | **DONE (research driver); PENDING on the shipped stub** | enwik9 encode peak 5.63 GiB (`evidence/runs/t2_gate/tune53.jsonl.peak_rss_bytes`). The stub reports its own projection before starting: 6.53 GiB encode / 5.59 GiB decode against an 8 GiB internal budget. |
| B2 | peak RAM ≤ 10 GB, decode | **DONE (research driver); PENDING on the stub** | ≈5.6 GiB observed. |
| B3 | temp disk ≤ 100 GB | **DONE** | The program opens only its arguments; no scratch files. |
| B4 | runtime `< 70,000/T` h per program | **DONE within a wide margin** | ≈58 min encode, ≈49 min decode, single-threaded. The strictest published reading (AMD 8-core, `T = 8228`) allows 8.5 h. |
| B5 | no GPU in the judged path | **DONE** | No GPU runtime is linked; `readelf -d` shows `libc.so.6` only. |
| B6 | determinism across fresh builds | **DONE** | Two rebuilds of the stub are byte-identical, and the archives they produce are byte-identical (`tools/measure_tune_table_cost.sh` reports `measurement_stable=yes`). |
| B7 | the scored build cannot allocate without bound from a forged header | **DONE** | `MAX_TABLE_SCALE` clamps the scale; the corruption court asserts it for **all 256** values of the `tune` byte. |

## C. Self-containment and spirit

| # | requirement | status | evidence |
|---|---|---|---|
| C1 | no network, no external files, no additional installations | **DONE (to be re-proven with `env -i` on the shipped stub)** | The scored path has **zero dependencies** (`cargo tree`), links only libc, and opens only its own image (SFX form) or its two arguments. |
| C2 | no environment dependence | **OPEN** | The stub honours `ZENTROPY_OUT` for the output filename, defaulting to `data9`. Must be shown that no *coded decision* depends on it; the safer fix is to drop the variable. |
| C3 | no corpus concealed under obfuscation; no hash-lookup tricks | **DONE** | The payload is entropy-coded model output. The only corpus-derived information in the submitted bytes is the archive header, the charged token dictionary and the model's learned parameters, each counted in `S`; `ALGORITHM.md` §3 documents each. |
| C4 | the article-layout permutation is a transform, not hidden knowledge | **DONE** | The permutation is derived from page ids that travel in the corpus, costs 0 bytes, and the decoder restores by sorting on those ids. Identity control exactly 0 B; shuffle control +25,519 B. |
| C5 | published under an OSI-approved licence | **DONE** | MIT (`LICENSE-MIT`), same in `Cargo.toml`, published on crates.io. |
| C6 | licence inventory complete | **DONE** | [`LICENCE_INVENTORY.md`](LICENCE_INVENTORY.md): zero third-party crates in the scored path; libc is a system library and is not redistributed. |

## D. Reproducibility

| # | requirement | status | evidence |
|---|---|---|---|
| D1 | builds from a clean checkout with no network | **OPEN** | The scored path has no dependencies, so `--offline` should work; it has not been tested from a clean clone. The pinned toolchain (`nightly-2026-07-24`) must be present, or the stable fallback used (400,816 B stub, documented). |
| D2 | build instructions tested from clean | **OPEN** | Same run as D1. |
| D3 | the algorithm is documented for a reviewer | **DONE** | [`ALGORITHM.md`](ALGORITHM.md). |

## E. Portability

| # | requirement | status | evidence |
|---|---|---|---|
| E1 | runs on the judge's machine | **OPEN — the most material risk** | The dynamic build requires **glibc ≥ 2.34** on x86-64 Linux. The rules' Linux test machine dates from 2021 and "may change without notice". Remedies in order of preference: a statically linked musl build (must be shown archive-identical), building on an older host, or the source-zip submission form. See `LICENCE_INVENTORY.md` §4. |
| E2 | not dependent on host CPU features | **DONE** | No SIMD in the scored path; AVX2 was measured slower and rejected. |
| E3 | not dependent on a floating-point library | **DONE** | The stub's symbol table contains no libm call: `nm -D --undefined-only <stub> | grep -iE 'log|exp|pow|sqrt|round'` is empty. The trainer that brought in `log2f` is gated out of `accepted`. |

## F. The score

| quantity | value |
|---|---|
| accepted record `L` (`fx2-cmix`) | 110,793,128 |
| 1% gate `floor(0.99 L)` | **109,685,196** |
| strongest credible pending (`fx2-cmix-transformer`) | ≈100,424,672 |
| internal moonshot target (not a claim) | 95,000,000 |
| Zentropy `S` at the Phase-11 result (tune 53) | `2 × 112,264 + 165,344,019` = **165,568,547** |
| Zentropy `S` after Phase 12.1 stub reclamation | `2 × 105,536 + 165,344,019` = **165,555,091** |
| **gap to the gate** | **≈55,869,895 B (≈55.9 MB)** |

The stub reclamation moved `S` by **13,712 B**. The remaining gap is a *ratio*
problem, not a byte-counting problem, and no part of this document should be read
as suggesting otherwise.

## G. The next three things

1. **Run the authority packaging on enwik9** with the shipped stub, which closes
   A1, B1 and B2 (≈1.7 h).
2. **Close the portability risk** (E1) — a musl static build proven
   archive-identical, or a decision to submit source.
3. **Re-bracket the mixer learning rate** once the adaptation-rate gate lands, per
   Phase 9's rule that the LR optimum follows the predictor.
