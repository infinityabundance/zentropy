# Licence inventory

> **Why this file exists.** The Hutter rules require the source to be published
> under an OSI-approved licence before payout, and a submission must be
> self-contained: no library may be fetched, and nothing non-free may be
> statically linked into the judged program. This is the enumeration, and every
> row is a measurement rather than a recollection.

Measured on the shipped stub at revision `4caa5de`, built with the pinned
toolchain (see `tools/package_sfx.sh`) and `--features accepted,submission`.

## 1. Our own code

| component | licence | file |
|---|---|---|
| Zentropy (all of `src/`, `tools/`, `docs/`) | **MIT** | [`LICENSE-MIT`](../LICENSE-MIT) |

MIT is OSI-approved, so the publication requirement is satisfied by the
repository's existing licence. The crates.io release (`zentropy`) carries the
same `license = "MIT"` in `Cargo.toml`.

## 2. Third-party crates in the scored path — **none**

```
$ cargo tree --no-default-features --features accepted,submission
zentropy v0.0.7 (/mnt/1tb_kingston/zentropy)
```

The scored path has **zero dependencies**. This is deliberate and load-bearing
for three separate requirements:

1. **`S`.** Every linked byte is charged twice (both packaging forms charge the
   program twice), so a linked runtime is a direct score cost.
2. **Self-containment.** A submission must run with no external files. A
   dependency-free static link removes the question entirely.
3. **Licence inventory.** With no third-party crates, the inventory below is
   complete by construction rather than by audit.

`rayon` is a normal dependency but is **optional and outside `accepted`**
(`parallel` feature, research plane only). It does not appear in the tree above,
and `docs/PARALLELISM_DECISION.md` records why it is not in the scored build.

The research *driver* is a different artefact from the submission and is not
covered by this inventory, because it is never submitted.

## 3. What the stub actually links

```
$ ldd target/x86_64-unknown-linux-gnu/submission/zentropy-sfx
    linux-vdso.so.1
    libc.so.6
    /lib64/ld-linux-x86-64.so.2

$ readelf -d <stub> | grep NEEDED
  (NEEDED)  Shared library: [libc.so.6]
```

| component | licence | notes |
|---|---|---|
| Rust standard library (`std`, `panic_abort`), compiled in via `-Z build-std=std,panic_abort` | **MIT OR Apache-2.0** (dual) | shipped source; both are OSI-approved |
| the Rust core/alloc crates it builds on | **MIT OR Apache-2.0** (dual) | same |
| glibc (`libc.so.6`) | **LGPL-2.1-or-later** | *system* library, dynamically linked, never redistributed |
| `linux-vdso.so.1`, `ld-linux-x86-64.so.2` | kernel/loader interface | provided by the host OS |

Notes that matter for the rules:

- **No `libm` any more.** Until Phase 12.1 the stub linked `libm.so.6` for a
  single `log2f` call that came from the offline trainer. That is now gated out
  (`docs/RESOURCE_CLOSURE.md` §10.1), so the only shared object besides the
  loader interface is libc. The check is one command:
  `nm -D --undefined-only <stub> | grep -iE 'log|exp|pow|sqrt|round'` must be
  empty.
- **glibc is not redistributed.** The submission ships an ELF that *requires*
  libc at run time; no glibc code is embedded, so no LGPL relinking obligation
  attaches. This is the ordinary case for every dynamically linked program.
- **No GPL, no non-free, no source-available-only** component appears anywhere in
  the shipped artefact.

## 4. Portability, and the one real caveat

`ldd --version` on the build host reads glibc **2.44**, and the stub's symbol
versions require up to `GLIBC_2.34`
(`readelf -sW <stub> | grep -oE 'GLIBC_[0-9.]+' | sort -uV | tail -1`). That
means the dynamic build needs a host with **glibc ≥ 2.34** — Ubuntu 22.04+,
Debian 12+, RHEL 9+.

The rules say the test machines "may change without notice" and list a 2021-era
Linux machine, which could be older than that. This is therefore an **open item**
for Phase 12, tracked in [`SUBMISSION_CHECKLIST.md`](SUBMISSION_CHECKLIST.md):

- **Preferred remedy:** a statically linked `x86_64-unknown-linux-musl` build,
  which has no glibc dependency at all and would run on any x86-64 Linux. It must
  be shown to produce a **byte-identical archive** before it can replace the
  glibc build, because a different libc is exactly the kind of change that can
  move a floating-point or libc-dependent result.
- **Fallback:** the `SCrt1.o`/loader requirement is a property of the host's
  glibc, so building on an older host lowers the symbol versions demanded.
- **Second fallback:** the rules also accept a source zip plus makefile, which
  removes the prebuilt-binary question entirely (at the cost of requiring a Rust
  toolchain on the judge's machine).

**Nothing in this file is a claim that the artefact has been run on the judge's
machine.** It records what the artefact requires.

## 5. How to reproduce this inventory

```sh
cargo tree --no-default-features --features accepted,submission   # expect: zentropy only
ldd    target/x86_64-unknown-linux-gnu/submission/zentropy-sfx    # expect: libc only
readelf -d target/x86_64-unknown-linux-gnu/submission/zentropy-sfx | grep NEEDED
nm -D --undefined-only target/x86_64-unknown-linux-gnu/submission/zentropy-sfx \
  | grep -iE 'log|exp|pow|sqrt|round'                             # expect: empty
readelf -sW target/x86_64-unknown-linux-gnu/submission/zentropy-sfx \
  | grep -oE 'GLIBC_[0-9.]+' | sort -uV | tail -1                 # expect: GLIBC_2.34
```
