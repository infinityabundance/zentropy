# Competitive Baseline

> Retrieved 2026-09-11 from <https://hutter1.net/prize/> and
> <https://mattmahoney.net/dc/text.html>. Pinned here so the project's targets
> are auditable and cannot silently drift.

This document answers one question: **what number must Zentropy beat, and what
is the frontier doing?** The architecture must never hardcode a target; the
gate is derived dynamically (see [`Targets`](../src/score/mod.rs)).

## 1. The accepted record (T0)

| Field | Value |
|---|---|
| Author(s) | Kaido Orav & Byron Knoll |
| Date accepted | 2024-10-08 |
| Decompressor | `fx2-cmix` |
| `archive9` | 110,351,665 |
| compressor | 441,468 |
| **Total `S` = `L`** | **110,793,128** |
| Compression factor | 9.03 |
| Improvement over predecessor | 1.59% |
| Award | 7,950 € |

The prize gate against T0 is therefore:

```
gate(T0) = floor(0.99 × 110,793,128) = 109,685,196 bytes
```

## 2. History of accepted records (enwik9)

| Date | Author | Decompressor | Total size | % improvement |
|---|---|---|---|---|
| 2024-09-03 | Kaido Orav & Byron Knoll | `fx2-cmix` | 110,793,128 | 1.59% |
| 2024-02-02 | Kaido Orav | `fx-cmix` | 112,578,322 | 1.38% |
| 2023-07-16 | Saurabh Kumar | `fast cmix` | 114,156,155 | 1.04% |
| 2021-05-31 | Artemiy Margaritov | `starlit` | 114,951,433 | 1.1% |
| 2019-07-04 | Alexander Rhatushnyak | `phda9 v1.8` | 116,673,681 | pre-prize |
| 2017-11-04 | Alexander Rhatushnyak | `phda9` (enwik8) | 15,284,944 | 4.17% |

## 3. Pending / frontier (T1)

These are **not** accepted records; they are the frontier the project must
watch. A submission may be pending verification or public comment for 30+ days.
`fx2-cmix-transformer` was pending final processing/comment as of retrieval.

LTCB snapshot (compressed `enwik9`, decompressor, total), front of table:

| Program | `enwik9` | prog | total | alg | note |
|---|---|---|---|---|---|
| `fx2-cmix-transformer` | 96,996,198 | 0 (xd) | 96,996,198 | Tr | pending; 6M-param transformer fed by PPM; no GPU to run |
| `tufazip 0.1.0` | 105,924,360 | 54,709 | 105,979,069 | Tr | transformer + CUDA preprocessor |
| `altxs 1.0.0` | 93,434,410 | 13,490,401 | 106,924,811 | Tr | cmix-lex + external transformer weights; GPU required |
| `nncp v3.2` | 106,632,363 | 628,955 | 107,261,318 | Tr | transformer predictor, dictionary preprocessing |
| `fx-deepmix` | 107,828,411 | 0 | 107,828,411 | LSTM | hoists reconstructible Wikipedia fields |
| `cmix-obias` | 108,009,834 | 0 | 108,009,834 | LSTM | |
| `cmix v21` | 107,963,380 | 281,387 | 108,244,767 | CM | |
| `forge-cmix v2` | 109,079,343 | 0 | 109,079,343 | LSTM | |
| `cmix-lex` | 109,190,109 | 470,599 | 109,671,639 | LSTM | fxcm_v26 heritage |
| `fx2-cmix` | 110,351,665 | 441,468 | **110,793,128** | LSTM | accepted T0 |
| `starlit` | 114,951,433 | 0 | 114,951,433 | CM | article reordering + reduced cmix |
| `phda9 1.8` | 116,544,849 | 42,944 | 116,587,793 | CM | |
| `paq8px_v206fix1 -12L` | 124,696,410 | 402,949 | 125,099,359 | CM | |
| `zpaq 6.42 -m s10.0.5fmax6` | 142,252,605 | 4,760 | 142,257,365 | CM | |
| `bsc 3.25` | 163,884,462 | 74,297 | 163,958,759 | BWT | |
| `bzip3 -b 511` | 169,990,721 | 368,033 | 170,358,754 | BWT | |
| `bzip2 1.0.2 -9` | 253,977,839 | 30,036 | 254,007,875 | BWT | |
| `xz 5.2.1` (1 GiB dict) | 197,331,816 | 36,752 | 197,368,568 | LZ77 | |
| `zstd 0.6.0 -22 --ultra` | 215,674,670 | 69,687 | 215,744,357 | LZ77 | |
| `brotli` (2016) `-q 11 -w 24` | 223,597,884 | 542,385 | 224,140,269 | LZ77 | |
| `gzip 1.3.5 -9` | 322,591,995 | 38,801 | 322,630,796 | LZ77 | |

**T1 (conservative)** = `100,424,672`, derived from
`fx2-cmix-transformer`'s 96,996,198 archive plus its 3,426,642-byte compressor
(the headline Hutter score reported was 100,424,672; the small difference is the
compressor build used in the scored run). If T1 is accepted as the record, the
new gate is `floor(0.99 × 100,424,672) = 99,420,425`.

## 4. Targets

| Target | Value | Meaning |
|---|---|---|
| **T0** | 110,793,128 | Accepted official record `L`; gate = 109,685,196 |
| **T1** | 100,424,672 | Strongest credible pending frontier |
| **T2** | 95,000,000 | Internal engineering moonshot — *not* a claim of attainability |

The project recomputes the gate from the latest accepted record; T1 and T2 are
informational.

## 5. What the frontier teaches

1. **Transformers win only when charged correctly.** The 2026 leader's model is
   ~6M parameters, pretrained on enwik9, quantised/compressed because its bytes
   count against `S`. The win is *compression gain per scored model byte*, not
   ML benchmark quality.
2. **The transformer corrects a classical predictor.** It is fed predictions
   from the PPM model, so it spends capacity on innovation the conventional
   predictor did not explain. This is exactly the VOLE-Audio decomposition:
   `observation = deterministic explanation + entropy-coded innovation`.
3. **Structure is hoisted, not predicted.** `fx-deepmix` lifts reconstructible
   Wikipedia fields out of the modelled stream and adds grammar/revision-aware
   modelling for metadata.
4. **Offline search is free; only its result is charged.** `starlit` reorders
   articles by expensive offline search, then restores the original order by
   sorting titles alphabetically. The search machinery never enters `S`.
5. **Negative-value models are deleted.** The cmix lineage removes predictors
   whose CPU/code cost exceeds the compression they earn. We make this a
   first-class law.

## 6. Baseline harness requirements

For each of `gzip -9`, `brotli -q 11`, `zstd -19/--ultra -22`, `xz -9e`,
`bzip2 -9`, `bzip3 -b511`, `zpaq`, `zstd --long`, and (where practical) `cmix`
lineage and `nncp`, record `archive bytes`, `program/decompressor bytes` where
meaningful, `wall time`, `RAM`, `disk` — and always report a **Hutter-equivalent
score**, never a bare ratio. See `tools/baseline.sh` and
`evidence/baseline/`.

## 7. Method caveat

Ratios on enwik8 do **not** predict enwik9 ordering: several programs change
rank because their model saturates memory. The project therefore reports every
mechanism on the ladder (`enwik6 → enwik7 → enwik8 → regions → enwik9`) and only
promotes on the full corpus at milestone gates.
