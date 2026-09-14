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
| `accepted` − `mem-guard` + `mem-floor` | 394,128 |
| `accepted` − `mem-guard` | 394,096 |

Therefore:

- **`mem-guard` costs ~5.5–5.8 KB** (399,888 − 394,096 = 5,792 B; 399,632 − 394,128
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
