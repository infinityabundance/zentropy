# OOM protection: what is guarded, by what, and what it costs

> **Why this document exists.** A long run that dies at 87% costs ~45 minutes of
> encode, and on a shared workstation an unguarded run can push the user's session
> into swap. Both failure modes were observed. This file records the layers, the
> exact behaviour, and — because `S` is authority — the measured executable cost of
> each piece that reaches the scored binary.

## 1. The three layers

| # | layer | plane | mechanism | fails how |
|---|---|---|---|---|
| 1 | startup, judged-safe | **scored** (`mem-guard`) | `memory::budget` = `min(3/4·MemAvailable, 8 GiB)`; refuse to start if the projection exceeds it | clean error, no work done |
| 2 | startup, workstation-safe | research | `memory::research_budget` = layer 1 minus a 4 GiB reserve | same |
| 3 | runtime floor | **research only** (`mem-floor`) | every MiB coded, re-check `MemAvailable`; abort after a sustained breach | abort with a diagnostic |

Layers 1–2 are cheap and a refusal is always safe. Layer 3 is different in kind:
it is the only part that can abort a run *in flight*, and on the judged path that
would turn a successful reconstruction into a failure. So the judged decoder never
arms it — `enable_runtime_guard` is called only from `src/bin/zentropy.rs` (the
research driver), never from `zentropy-sfx`.

There is also a hard kernel ceiling outside the process: `tools/run_guarded.sh`
applies `RLIMIT_AS` and takes a single-heavy-run lock (`ZENTROPY_ALLOW_CONCURRENT=1`
to opt out).

## 2. The floor pauses; it does not abort

`enforce_runtime_floor` **suspends** the run when `MemAvailable` falls below the
floor, sleeping `FLOOR_POLL` (5 s) between re-checks and resuming the instant
memory recovers. It gives up only after `FLOOR_PATIENCE` (600 s) of *sustained*
pressure, which is a last resort that releases the run's memory for the rest of
the machine.

Pausing is legitimate, not a trick, because coding is a pure function of the input
and the model state: no clock, no randomness, no shared mutable state. The same
bytes are coded the same way before and after the sleep, and the decoder re-running
the same bytes makes the same decisions. It is the property that makes a pause
*correct* where an abort was merely *safe*.

### 2.1 Why: two incidents, and what was actually established

**Incident 1** (single-sample rule, ~08:45). Five concurrent enwik9 gates were
killed at 87% while a `cargo build` ran alongside them. Whether that dip was
sustained was not recorded, so it did not by itself justify a design change.

**Incident 2** (the debounce, ~09:10). The gates were relaunched and 3-consecutive-
sample debouncing was in place. All five died again, and this time the diagnostic
was conclusive:

```text
memory floor breached: 1.12 GiB available, floor 2.00 GiB,
for 3 consecutive samples (3.00 MiB coded)
```

So the dip **was sustained** — a longer patience window alone would not have saved
them. That is what ruled out "debounce harder" and selected "pause instead of
deciding".

### 2.2 The workstation, not the guard, is the pressure

Both incidents trace to the machine, not to Zentropy. Measured while five gates ran:

| process | RSS |
|---|---|
| `xmllint` | **46.7 GB** |
| `zed-editor` | 8.1 GB |
| `xmllint` (second) | 3.4 GB |
| five `zentropy eval` gates | ~7.6 GB total |

`SwapFree` was **1.95 GB of 131.5 GB** and `Committed_AS` 239 GB, so the machine has
essentially no slack: an unrelated process spiking can drop `MemAvailable` from
62 GB to ~1 GB. Under those conditions the floor's job is to keep a long run alive
across someone else's spike, which is exactly what pausing does. Note also that
`free -g` reported "available 1" at a moment when `MemAvailable` read 8.98 GB —
quote `/proc/meminfo`, not the rounded summary.

### 2.3 Operational rule that still stands

**Do not run a heavy experiment while gates are in flight.** Pausing makes our runs
resilient, but it does not create memory. A 5.5 GB `layout --bits 28` run alongside
five gates is a self-inflicted version of the same problem.

`floor_action` (pure, in `src/memory.rs`) holds the decision and is unit-tested for:
immediate pause on a dip, immediate resume on recovery however long it took,
inclusive comparison at exactly the floor, no abort before the patience window, and
abort at it.

## 3. What is *not* guarded

- **The judged decoder never aborts.** No floor, no refusal mid-stream, no
  environment dependence. A reconstruction either completes exactly or the archive
  is malformed.
- **No wall-clock or host-specific behaviour** enters any decision.

## 4. Measured executable cost (A31: measured, never estimated)

`opt-level="z"`, `lto="fat"`, `codegen-units=1`, stripped. All figures from
`RUSTFLAGS="" cargo build --profile submission --no-default-features --features …`,
rebuilt twice per row and stable.

| build | stub bytes |
|---|---|
| `accepted` + `mem-floor` | 399,632 |
| **`accepted`** (the scored set; floor implementation gated out) | **399,888** |

> **Superseded in Phase 11.** These four rows are the *stable* build, measured before
> the `panic_immediate_abort` change; the shipped stub is now **111,888 B** and the
> `mem-guard` marginal is **5,520 B** there. The relative findings (mem-guard real,
> mem-floor noise-level) are unchanged; see
> [`RESOURCE_CLOSURE.md`](RESOURCE_CLOSURE.md) §7.
| `accepted` − `mem-guard` + `mem-floor` | 394,128 |
| `accepted` − `mem-guard` | 394,096 |

Therefore:

- **`mem-guard` costs ~5.5–5.8 KB** in the stable build (**5,520 B** in the shipped
  one) (399,888 − 394,096 = 5,792 B; 399,632 − 394,128
  = 5,504 B). This is a real, reproducible, same-sign cost and is the one genuinely
  interesting Phase-11 target in this document.
- **`mem-floor`'s marginal cost is noise-level and sign-inconsistent**: −256 B in the
  `mem-guard` configuration, +32 B without it. `S` therefore provides *no* evidence
  for carrying it, and the architectural argument (a judged decoder must not carry an
  abort path) decides: it stays outside `accepted`.

> **The general lesson, which cost the project its first 256 B scare:** at
> `opt-level="z"` the artifact size is a *layout* property, not a sum of feature
> costs. Removing an inert call made the stub **256 B larger** (399,632 → 399,888,
> three identical repetitions). Keeping the call site and gating only the body did
> not restore the smaller size, which is how we know the lever was the body's effect
> on the inliner rather than the call itself. **Do not quote a sub-kilobyte stub
> delta as a mechanism price.** Re-measure on the tree that will ship. This is the
> same class of error as the original 543 B → 1,728 B structural-hoist estimate.

## 5. Phase-11 targets that fall out of this

1. **`mem-guard` (~5.5–5.8 KB).** The startup check re-derives the model's memory
   with `projected_encode`/`projected_decode` → `model_bytes` →
   `ACCEPTED_METHOD.config_for(n).memory_bytes()`. The encode/decode path *already*
   builds that config, so the check can move to the point where the information
   already exists, which should cost close to nothing. Do not simply delete it: the
   refusal is the scored safety property; make it cheaper.
2. **Re-measure the whole table above** after the rejected-method dispatch is
   stripped, since every number here is layout-dependent.

## 6. The test plane — the fourth heavy process, and the one that was unguarded

Layers 1–3 above all guard a *coding run*. `cargo test` is a heavy process of a
completely different shape and had no guard at all. On 2026-09-14 that gap took
the user's editor down: an edit to `Cargo.toml` made rust-analyzer start a
`cargo check` behind the editor while two enwik9 gates held ~6.8 GB, and the
kernel picked the editor as the OOM victim.

The lesson is that "the codec is guarded" is not the same as "the project is
guarded". Four bounds now cover the test plane, and none of them touches the
scored artifact:

| # | bound | where | what it actually stops |
|---|---|---|---|
| 1 | `[build] jobs = 2` | `.cargo/config.toml` | rustc fan-out — **including rust-analyzer's `cargo check`**, which is what spiked the machine |
| 2 | `RUST_TEST_THREADS = "2"` | `.cargo/config.toml` | libtest's default one-thread-per-core, which multiplies every test's allocation by the core count |
| 3 | `RLIMIT_AS` + single-heavy-run lock | `tools/test_guarded.sh` → `tools/run_guarded.sh` | a runaway test binary, and a court running beside a gate |
| 4 | `TEST_ALLOCATION_CEILING` (1 GiB) | `memory::assert_test_budget`, called from `Predictor::new` | a test that grew a large fixture — loud panic naming the budget, instead of a machine-wide OOM kill |

Bound 4 is deliberately placed at the **single point where a model is
allocated** rather than on a list of tests, so it covers tests added later and
tests nobody remembered to annotate. It is `#[cfg(test)]`: absent from every
shipped binary, so it cannot affect `S`.

Bounds 1 and 2 are `[env]`/`[build]` entries rather than wrapper flags on
purpose: cargo does not overwrite an already-exported `RUST_TEST_THREADS` unless
`force = true`, and `cargo --jobs N` beats `[build] jobs`, so a deliberate wide
build is still one flag away. `tools/courts.sh` runs the courts through
`tools/test_guarded.sh` for the same reason: the guard should be the default,
not something a caller has to remember.

### 6.1 It immediately found two real defects

Bound 4 was not decoration. On its first run it failed `tuning_variants_roundtrip`
with an allocation of **1.26 GiB** — a test that walks the whole 0..=255 `tune`
space on an 8.8 KB fixture. Tracing that turned up a genuine hole in the *scored*
decoder, not a test problem:

* T2's table scale lives in the high nibble of the `tune` byte, and `tune` comes
  out of the archive header. Unclamped, a single forged byte could ask the
  decoder for 2^(18+15)-slot tables — an attacker-chosen allocation, exactly the
  "allocates without bound" failure the corruption court forbids.
* `mem-guard` projected the *accepted* configuration regardless of what the
  archive declared, so a forged header was cleared by a guard that had measured a
  different model.

Both are fixed in the T2 adoption: the scale is clamped at the largest value the
project runs (`context::MAX_TABLE_SCALE`), which makes the accepted geometry the
worst case for *any* archive; and the projection now follows the archive's own
header (`archive::peek_header` → `memory::projected_decode_for`). The corruption
court asserts the bound for **all 256** values of the `tune` byte rather than
sampling a few.

The general lesson is the same one as the 543 B → 1,728 B estimate: a guard is
only a guard if it is run, and a bound is only a bound if it is asserted about
the *actual* input. The test-plane guard found a decoder hole because it was
applied to the decoder's own code path.

### 6.2 The unguarded thing was the operator, not the editor

Three times a long session ended with the editor being killed, and twice it was
attributed to the codec because the codec was the most recent suspect. That was
wrong in an important way: it led to *editing the user's editor configuration*,
which is not this repository's business and had to be reverted. The honest account:

* Measured on 2026-09-15, before starting any heavy run, `zed-editor` was
  **24.6 GB** resident and `target/` held **28,359 files**. So the editor was
  already large and the machine was already close to its limit.
* **The commands that tipped it were the operator's.** `tools/run_guarded.sh`
  covered the coding runs, and only those. Every `cargo build`, every
  `cargo test`, and every shell loop over several gates ran **outside any
  ceiling** — and a `for` loop launching three gates is three unguarded
  processes, not one.

So the fix belongs on the operator's side, and it is now a rule with a tool:
**every terminal command runs through `tools/memcap.sh <gib> <command>`.**

### 6.3 `memcap.sh`: what was tried, and what was rejected

The first version used a systemd scope (`systemd-run --user --scope -p
MemoryMax=1G`), which is strictly stronger than `RLIMIT_AS` because it bounds the
*total* of a command tree rather than each process. It was removed, because it was
tested rather than trusted and it **silently did not enforce**: a 3 GiB allocation
succeeded inside a 1 GiB scope on this machine. A guard that reports success while
enforcing nothing is worse than no guard.

The shipped mechanism is `RLIMIT_AS` via `ulimit -v`, which was verified to
actually stop an over-budget allocation:

```text
tools/memcap.sh 1 python3 -c "bytearray(3*1024**3)"   -> MemoryError, exit 1
tools/memcap.sh 4 python3 -c "bytearray(1*1024**3)"   -> allocated, exit 0
```

The limit is per process and is inherited, so a shell that spawns children caps
each of them; the total is bounded by (processes × cap), which is why the cap is
paired with a bounded process count (`.cargo/config.toml` sets `jobs = 2`).

Recommended caps: **1 GiB** for trivial commands, **8 GiB** for anything that
compiles, **10 GiB** for anything that codes.

### 6.4 Every heavy path, not just the ones we remembered

The hard `RLIMIT_AS` ceiling was applied by the gate wrappers but **not** by
`package_sfx.sh` or `make_submission.sh`, which invoke the scored stub directly —
so the enwik9 authority run, the single heaviest thing this project does, ran
under only the stub's own startup projection. Both now route their stub
invocations through `tools/run_guarded.sh` (`ZENTROPY_STUB_CAP_GIB`, default 9 GiB,
against a measured 5.96 GiB peak). An estimate is a guard only if a kernel ceiling
backs it up.

### 6.5 What is still not guarded

The editor's *baseline* footprint is outside the repository's control, and the
repository must not try to configure it. What the operator controls is now capped,
so this project's own commands can no longer be the trigger; `tools/watch_run.sh`
will show which process is large if it happens again.
