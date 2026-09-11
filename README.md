# Zentropy

**Find the smallest executable explanation of `enwik9`.** Persist the
explanation, encode only the irreducible innovation, and charge every
explanatory mechanism for every byte required to reconstruct the exact original.

Zentropy is a legitimate, rules-compliant [Hutter Prize](https://hutter1.net/prize/)
contender written in Rust. Its objective is not "an interesting compressor"; it
is a valid submission that beats the strongest applicable predecessor by enough
margin to win.

```
S = submitted_compressor_bytes + self_extracting_archive_bytes
subject to:  decode(archive9) == enwik9   (byte-for-byte)
             and every Hutter resource, portability, publication and
             self-containment constraint.
```

`S` is authority. Bits-per-byte, compression ratio and model cross-entropy are
diagnostics. A mechanism is admitted only when its **complete** marginal cost
`ΔS < 0`.

## Status at a glance

| | |
|---|---|
| Exactness | Every transform and archive round-trips exactly; 51 unit/property tests + 7 scripted courts pass |
| enwik9 | exact full-corpus reconstruction at **182,949,204 bytes (1.4636 bpc)**, 3.58 GB peak RAM |
| enwik8 | exact reconstruction at **22,372,738 bytes (1.7898 bpc)** (accepted config, Optimization A) |
| Reference | gzip 36.4 MB · bzip2 29.0 MB · xz -9e 24.8 MB · brotli 25.7 MB · **zentropy 22.5 MB** |
| Submission path | scored stub (313,792 B) compresses/decompresses; self-extracting `archive9` reconstructs with no inputs |
| Phase | 0–2 measured; 3–12 in progress / proposed (see the architecture doc) |

> No claim of competitiveness against the 110 MB record is made yet. The
> Phase-2 floor exists so every later mechanism can be attributed by ablation.

## Quick start

```sh
# exactness + property tests
cargo test

# end-to-end self test
cargo build --release
./target/release/zentropy selftest

# corpus provenance
./target/release/zentropy hash evidence/corpus/enwik8

# reversible Wikipedia IR statistics
./target/release/zentropy tokenize evidence/corpus/enwik6

# compression / reconstruction with a full report
./target/release/zentropy bench evidence/corpus/enwik7

# leave-one-out ablation of the word experts
./target/release/zentropy ablate evidence/corpus/enwik7

# build the submission-plane artefact and measure the real Hutter split
sh tools/package_sfx.sh evidence/corpus/enwik6

# convene all automated courts
sh tools/courts.sh
```

## Repository map

```
docs/       HUTTER_RULES, COMPETITIVE_BASELINE, ZENTROPY_ARCHITECTURE,
            PRIOR_ART_MECHANISMS, STACK_TECH_TRANSFER
src/        single Rust package, zero external dependencies
  corpus/   provenance, SHA-256 (in-tree), ladder
  score/    S, gate, resource limits (single source of truth)
  evidence/ immutable receipts, Gemel research memory
  entropy/  binary range coder, rANS side-stream backend
  mixer/    squash/stretch, adaptive mixer, APM/SSE
  context/  context models, word/bigram experts, match model
  ir/       ZIR-0 exactly-reversible Wikipedia tokenisation
  archive/  the scored container + SFX packing format
  bin/      `zentropy` (driver), `zentropy-sfx` (submission stub)
tools/      baseline, courts, packaging harnesses
evidence/   pinned digests, run receipts, baselines
research/   third-party reference material (gitignored)
```

## Why zero dependencies

Binary size is a scored metric, determinism must be *demonstrated*, and the
licence inventory must be trivial. The crate carries its own SHA-256, range
coder, rANS and context-mixing stack. Nothing in the scored path is fetched,
linked, or assumed.

## Licence

MIT. See `LICENSE-MIT` and `Cargo.toml`. Any submission will ship under an
OSI-approved licence with a complete inventory, as the rules require.
