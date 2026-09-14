#!/bin/sh
# Run the test suite under the OOM guard.
#
# Why this exists (and why `.cargo/config.toml` exists alongside it)
# ------------------------------------------------------------------
# Every long *coding* run already goes through `tools/run_guarded.sh`, and the
# codec itself carries `mem-guard`. Tests had nothing:
#
#   * libtest defaults to one thread per core, so `cargo test` multiplies each
#     test's allocation by the core count;
#   * the build fans out to one rustc job per core, and rust-analyzer runs
#     `cargo check` on every edit — so an edit could start a many-GiB build
#     behind the editor. That is what killed the user's editor once.
#
# Three independent bounds, none of which touches the scored artifact:
#   1. `.cargo/config.toml` `[build] jobs = 2`        — bounds rustc, including
#      the build rust-analyzer triggers. This is the one that actually mattered.
#   2. `.cargo/config.toml` `RUST_TEST_THREADS = "2"` — bounds the harness.
#   3. this script: `RLIMIT_AS` plus the single-heavy-run lock, so tests cannot
#      run beside a gate and cannot exceed a hard address-space ceiling.
#   4. `memory::assert_test_budget`, called from `Predictor::new`, fails a test
#      that is about to allocate more than 1 GiB — loud and local.
#
# Usage: tools/test_guarded.sh [cargo test args...]
#   e.g. tools/test_guarded.sh --release
#        tools/test_guarded.sh --profile submission
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# Comfortably above rustc's own peak for this tree and far above the measured
# test-process peak, far below the machine. Measured, not guessed: see
# docs/MEMORY_GUARD.md §6.
CAP="${ZENTROPY_TEST_CAP_GIB:-8}"

cd "$ROOT"
exec "$ROOT/tools/run_guarded.sh" "$CAP" cargo test "$@"
