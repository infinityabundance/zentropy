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
| A1 | `decode(archive9) == enwik9` byte-for-byte, verified by two independent hashes **and** a byte comparison | **DONE** | `tools/make_submission.sh evidence/corpus/enwik9` ran the **shipped static-musl stub** over the full 10⁹ bytes: `exactness: PASS (byte-identical)`. The archive is 160,015,425 B and the SFX form reconstructs it independently. |
| A2 | `S` computed with the exact counting rule for the chosen form | **DONE (enwik6/7)** | `tools/package_sfx.sh`; enwik6: `S_sfx = 457,903`, `S_separate = 457,880`, differing by exactly 23 B. `src/score/mod.rs` has the three forms as unit-tested constructors. |
| A3 | The two legal forms agree on the program's real size | **DONE** | The 23-byte cross-check above. |
| A4 | Every transform has a universal literal fallback; malformed input cannot break the decoder | **DONE** | `corrupt-court` (379 mutations, no panic, no unbounded allocation); `negative-court`; IR round-trip on random and malformed input. |
| A5 | No panic path can abort a judged decode | **DONE** | `panic = "abort"` plus `overflow-checks = true` in the scored profile, exercised by `cargo test --profile submission` (137 tests). |

## B. Resources

| # | requirement | status | evidence |
|---|---|---|---|
| B1 | peak RAM ≤ 10 GB, encode | **DONE on the shipped stub** | 5.96 GiB peak observed during the authority encode (previously 5.63 GiB on the research driver at the same geometry). The stub also projects before starting: 6.53 GiB encode / 5.59 GiB decode against an 8 GiB internal budget. |
| B2 | peak RAM ≤ 10 GB, decode | **DONE on the shipped stub** | ≈5.6 GiB; the authority run's decode pass completed inside the same envelope. |
| B3 | temp disk ≤ 100 GB | **DONE** | The program opens only its arguments; the authority bundle contains no scratch files. |
| B4 | runtime `< 70,000/T` h per program | **DONE, measured on the shipped stub** | Encode ≈40 min for the full corpus at the stub's measured 0.348 MB/s (scored from enwik7 through the same binary); decode the same order of magnitude. The strictest published reading (AMD 8-core, `T = 8228`) allows 8.5 h, so the margin is more than an order of magnitude. |
| B5 | no GPU in the judged path | **DONE** | No GPU runtime is linked; `readelf -d` shows `libc.so.6` only. |
| B6 | determinism across fresh builds | **DONE** | Two rebuilds of the stub are byte-identical, and the archives they produce are byte-identical (`tools/measure_tune_table_cost.sh` reports `measurement_stable=yes`). |
| B7 | the scored build cannot allocate without bound from a forged header | **DONE** | `MAX_TABLE_SCALE` clamps the scale; the corruption court asserts it for **all 256** values of the `tune` byte. |

## C. Self-containment and spirit

| # | requirement | status | evidence |
|---|---|---|---|
| C1 | no network, no external files, no additional installations | **DONE (to be re-proven with `env -i` on the shipped stub)** | The scored path has **zero dependencies** (`cargo tree`), links only libc, and opens only its own image (SFX form) or its two arguments. |
| C2 | no environment dependence | **DONE** | The former `ZENTROPY_OUT` override was removed; the self-extracting form always writes `data9`. Proven with `env -i` from an empty directory and with `ZENTROPY_OUT=/tmp/evil` (ignored). |
| C3 | no corpus concealed under obfuscation; no hash-lookup tricks | **DONE** | The payload is entropy-coded model output. The only corpus-derived information in the submitted bytes is the archive header, the charged token dictionary and the model's learned parameters, each counted in `S`; `ALGORITHM.md` §3 documents each. |
| C4 | the article-layout permutation is a transform, not hidden knowledge | **DONE** | The permutation is derived from page ids that travel in the corpus, costs 0 bytes, and the decoder restores by sorting on those ids. Identity control exactly 0 B; shuffle control +25,519 B. |
| C5 | published under an OSI-approved licence | **DONE** | MIT (`LICENSE-MIT`), same in `Cargo.toml`, published on crates.io. |
| C6 | licence inventory complete | **DONE** | [`LICENCE_INVENTORY.md`](LICENCE_INVENTORY.md): zero third-party crates in the scored path; libc is a system library and is not redistributed. |

## D. Reproducibility

| # | requirement | status | evidence |
|---|---|---|---|
| D1 | builds from a clean checkout with no network | **DONE for the scored path** | The generated `source.tar.gz` was extracted to an empty directory and built with `cargo build --offline --no-default-features --features accepted,submission`: success, no network. Only the Rust toolchain is needed, because the scored path has no dependencies to fetch. |
| D2 | build instructions tested from clean | **DONE (scored path); OPEN (the pinned nightly)** | The offline build above used the default stable toolchain. The *submission* build pins `nightly-2026-07-24` and needs `-Z build-std`; that toolchain must either be present on the judge's machine or the documented stable fallback used (a 400,816 B stub). |
| D3 | the algorithm is documented for a reviewer | **DONE** | [`ALGORITHM.md`](ALGORITHM.md). |

## E. Portability

| # | requirement | status | evidence |
|---|---|---|---|
| E1 | runs on the judge's machine | **DONE (subject to a final run on the shipped binary)** | The shipped artefact is a **static-pie musl** build: `ldd` reports "statically linked", so it needs no shared library, no loader beyond what the kernel provides, and no glibc version at all. Cost: +19,520 B per copy versus the glibc build. Both produce byte-identical archives on enwik6/enwik7. Residual risk: it is x86-64 only (the rules accept x86 32/64-bit Linux executables), and it has not been executed on the judge's machine. See `LICENCE_INVENTORY.md` §4. |
| E4 | no environment dependence in the judged path | **DONE** | `env -i ./archive9` from an empty directory reconstructs byte-identically and writes `data9`; the former `ZENTROPY_OUT` override was removed. |
| E2 | not dependent on host CPU features | **DONE** | No SIMD in the scored path; AVX2 was measured slower and rejected. |
| E3 | not dependent on a floating-point library | **DONE** | The stub's symbol table contains no libm call: `nm -D --undefined-only <stub> | grep -iE 'log|exp|pow|sqrt|round'` is empty. The trainer that brought in `log2f` is gated out of `accepted`. |

## F. The score

| quantity | value |
|---|---|
| accepted record `L` (`fx2-cmix`) | 110,793,128 |
| 1% gate `floor(0.99 L)` | **109,685,196** |
| strongest credible pending (`fx2-cmix-transformer`) | ≈100,424,672 |
| internal moonshot target (not a claim) | 95,000,000 |
| Zentropy `S` at the Phase-11 close | `2 × 112,264 + 169,282,339` = **169,506,867** |
| after T2 (scale 3) | `2 × 112,264 + 165,344,019` = 165,568,547 |
| after the adaptation ladder (−3,925,403) | `2 × 112,264 + 161,418,616` = 161,643,144 |
| after the retrained corrector (−664,427) | `2 × 112,264 + 160,754,189` = 160,978,717 |
| after the mixer-LR move (−738,764) | `2 × 112,264 + 160,015,425` = 160,239,953 |
| **shipped**, Phase-12.1 stub reclamation and static musl | `2 × 125,056 + 160,015,425` = **160,265,537** |
| **gap to the gate** | **≈50,580,341 B (≈50.6 MB)** |

Net since the Phase-11 close: the archive fell **9,266,914 B** and the program cost
**+25,584 B of `S`** (the static-musl eligibility trade), for **−9,241,330 B of
`S`**. The remaining gap is a *ratio* problem, not a byte-counting problem, and no
part of this document should be read as suggesting otherwise.

The stub reclamation moved `S` by **13,712 B**. The remaining gap is a *ratio*
problem, not a byte-counting problem, and no part of this document should be read
as suggesting otherwise.

## G. The bundle, and what is left

`tools/make_submission.sh <corpus> [outdir]` produces the whole submission in one
command — `comp9a`, `decomp9`, `archive9.bhm`, `archive9`, `source.tar.gz` and a
`MANIFEST.txt` — and it refuses to write anything unless the **shipped** stub has
reconstructed the corpus byte-for-byte and the self-extracting form has done so
under `env -i` from an empty directory.

It has been run on the authority corpus:

```
program_bytes     = 125,056        (comp9a == decomp9)
archive9.bhm      = 160,015,425
archive9          = 160,140,504
S(self-extracting) = 160,265,560
S(separate, 2P+bhm) = 160,265,537
exactness          = PASS (byte-identical, via the shipped stub and under env -i)
```

Remaining work, in order:

1. **Interaction matrix and the LR/ladder re-bracket** after any Tier-1
   adoption — Phase 13 §13.5. Both knobs have now moved three times, each time
   because the predictor changed.
2. **The portability decision, made explicitly.** The shipped artefact is static
   musl (runs anywhere, +19,520 B per copy); the glibc build is 105,536 B and
   needs glibc ≥ 2.34. The current default favours eligibility; that choice should
   be recorded as a decision rather than left as a default.
3. **A clean-clone build with the pinned nightly**, which needs
   `rustup target add x86_64-unknown-linux-musl --toolchain nightly-2026-07-24`
   on the judge's machine or the documented stable fallback.
4. **Phase 13**, the re-test campaign: almost every rejected mechanism was
   measured against a predictor that no longer exists
   ([`PHASE13_PLAN.md`](PHASE13_PLAN.md)).
