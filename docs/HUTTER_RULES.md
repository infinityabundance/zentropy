# Hutter Prize — Rules of Record

> **Authority.** This file is a working transcription and interpretation of the
> primary sources. When it disagrees with the primary source, the primary
> source wins and this file is a bug. Retrieved 2026-09-11.

Primary sources:

- Rules of participation: <https://hutter1.net/prize/hrules.htm>
- Prize page / previous records: <https://hutter1.net/prize/>
- Large Text Compression Benchmark (LTCB): <https://mattmahoney.net/dc/text.html>

## 1. The task

Losslessly compress the 1 GB file `enwik9` such that the complete submission is
smaller than the preceding record, exactly reconstructing the original.

Concretely, one of:

| Form | Score `S` |
|---|---|
| Self-extracting archive | `len(comp9) + len(archive9)` |
| Separate compressor/decompressor | `len(comp9a) + 2·len(decomp9) + len(archive9.bhm)` |
| Same program for both | `len(comp9a) + len(decomp9) + len(archive9.bhm)` |

If `comp9a = decomp9`, the `2×` in the separate form reduces to `1×`. Note that
this still charges the single program **twice in total** — once as `comp9a` and
once as `decomp9` — i.e. `S = 2·len(P) + len(archive9.bhm)`. It is *not*
`len(P) + len(archive9.bhm)`. Concretely, a 313,792-byte program with a
276,263-byte archive scores `903,847`, not `590,055`. The three forms are
implemented as named, unit-tested constructors in
[`score`](../src/score/mod.rs).

Running `archive9` (with no input from other sources) must produce a file
byte-identical to `enwik9`.

## 2. `enwik9` provenance

- Defined as the first `10^9` bytes of the English Wikipedia XML dump of
  **2006-03-03** (`enwiki-20060303-pages-articles.xml`).
- Exact length: **1,000,000,000 bytes** (decimal). The FAQ clarifies 1 GB is
  `10^9`, not `2^30`.
- Distribution: <https://mattmahoney.net/dc/enwik9.zip>.
- Pinned digests measured during Phase 0 (recorded in
  `evidence/baseline/CORPUS.sha256`):

```
enwik8 (first 10^8 bytes)  2b49720ec4d78c3c9fabaee6e4179a5e997302b3a70029f30f2d582218c024a8
enwik9 (10^9 bytes)        159b85351e5f76e60cbe32e04c677847a9ecba3adc79addab6f4c6c7aa3744bc
enwik6 (first 10^6 bytes)  369b688978f649681136198fb96db14c1616756260c55fb4b65e9bc049552cad
enwik7 (first 10^7 bytes)  5985c81c39d927ae0e169625790ca4d9e7d1531270c8b09ad73176a375bb3d97
```

## 3. Resource limits

| Constraint | Value |
|---|---|
| Peak RAM | ≤ 10 GB |
| Temporary disk | ≤ 100 GB |
| Runtime | `< 70,000 / T` hours per program, `T` = machine Geekbench 5 single-core score |
| GPU | **not permitted** during the judged run |
| Cores | effectively single-core (the published machines' single-core `T` is used) |

With the published test machines:

| Machine | `T` (1 core) | Time limit |
|---|---|---|
| Intel i7-1165G7 2.79 GHz | ≈1427 | ≈49.05 h |
| AMD Ryzen 7 3.6 GHz | 1310 | ≈53.44 h |

`zentropy`'s [`ResourceLimits`](../src/score/mod.rs) uses `T = 1310` (the
slower machine) as the conservative default.

## 4. Portability and self-containment

- Windows or Linux, x86 32- or 64-bit executables.
- Must run without input from other sources: no files, network, dictionaries,
  or additional installations. Standard libraries for file I/O are allowed.
- In lieu of executables, a zip of source + makefile may be submitted (C++,
  Python, Assembler accepted; other languages considered if easily built and
  verified).
- Command-line option lengths count toward `S`.
- Decompressors may be tuned specifically to this benchmark and may reject or
  fail on any input other than `enwik9` / `archive9.bhm`.
- Source must be published under an OSI-approved licence before payout.

## 5. Award

```
Award = Z × (L − S) / L        Z = prize fund (currently 500,000 €)
```

- `L` = previous record for `S`; `S` = new record.
- `L` is updated to `S` after an award; `Z` does **not** decrease.
- **Minimum claim is 1% of `Z`** (currently 5,000 €), i.e. a submission must
  improve on `L` by at least 1%:
  `S ≤ floor(0.99 × L)`.
- Contributions are handled in submission order.
- At least 30 days of public comment before an award.
- If a 1%-criterion miss occurs, `L` is unchanged; the submission is ignored.

**Derived gate.** Against the accepted record `L = 110,793,128`:

```
gate = floor(0.99 × 110,793,128) = 109,685,196 bytes
```

`zentropy` recomputes this dynamically from the latest accepted record; see
[`Targets`](../src/score/mod.rs). It is never hardcoded into the architecture.

## 6. Spirit of the contest

Explicitly forbidden, in letter or in spirit:

- external network access;
- hidden files or undeclared runtime dictionaries;
- fetching models;
- operating-system-specific hidden knowledge;
- embedding the original corpus under cosmetic obfuscation;
- hash-lookup tricks that merely conceal corpus bytes;
- undefined-behaviour-dependent reconstruction;
- machine-specific floating-point behaviour that threatens replay;
- GPU dependency in the judged compressor/decompressor;
- results depending on unrecorded environment state.

Decompressors "secretly receiving any kind of outside information are
forbidden." Corpus-specific modelling, learned parameters, transforms,
dictionaries and ordering are permitted **only insofar as the rules currently
permit them and the complete submitted representation pays their byte cost.**

## 7. Interpretation policy

Whenever legality or spirit is unclear:

1. **STOP.**
2. Record the question as an open decision record.
3. Read the current primary rule source.
4. Construct the conservative interpretation.

We do not build a numerically winning result that risks disqualification.

## 8. Open questions / to re-verify before any submission

| # | Question | Conservative position |
|---|---|---|
| Q1 | Is an executable stub whose bytes are appended to the archive counted once or twice? | Count it in `len(comp9)` **and** `len(archive9)` exactly as the chosen submission form dictates; prefer the form that minimises `S`. |
| Q2 | May the compressor and decompressor share a single binary? | Yes, and it reduces the `2×` factor to `1×`; prefer shared code where scoring favours it. |
| Q3 | Are externally trained model weights "outside information"? | They are permitted only if they are *inside* the submitted bytes and charged to `S`; nothing may be fetched at judged time. |
| Q4 | Does `T` use single-core or multi-core Geekbench 5? | Assume single-core (conservative). |
| Q5 | May temporary files be created and deleted during the judged run? | Yes, up to 100 GB peak; they must not carry information not representable in the submission. |

## 9. Compliance checklist (per submission)

- [ ] `decode(archive9) == enwik9` byte-for-byte, verified by two independent
      hashes and a byte comparison.
- [ ] `S` computed with the exact counting rule for the chosen form.
- [ ] Peak RAM measured `< 10 GB`.
- [ ] Peak temp disk measured `< 100 GB`.
- [ ] Runtime measured on an eligible machine and scaled by `70,000/T`.
- [ ] No GPU in the judged path.
- [ ] No network, no external files, no environment dependence.
- [ ] Deterministic across at least two fresh eligible machines.
- [ ] Source published under an OSI licence; licence inventory complete.
- [ ] Algorithm document included; build instructions tested from clean.
