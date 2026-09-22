#!/bin/sh
# Run any command under a hard address-space ceiling (`RLIMIT_AS`).
#
# Why this exists
# ---------------
# `tools/run_guarded.sh` protects the coding runs. That was not enough, and the
# gap was not in the codec — **the operator's own commands were unguarded**. A
# `cargo build`, a `cargo test`, a shell loop over three gates: each can spike, and
# each ran outside any ceiling. On a workstation that already holds a large editor
# session, "outside any ceiling" *is* the failure mode.
#
# Why `RLIMIT_AS` and not a systemd scope
# ---------------------------------------
# `systemd-run --user --scope -p MemoryMax=1G` is available on this machine but
# **silently does not enforce**: a 3 GiB allocation succeeded inside a 1 GiB scope.
# A guard that reports success while enforcing nothing is worse than no guard, so
# that mechanism was removed rather than kept as a comfortable-looking default.
# `ulimit -v` was verified to actually stop an over-budget allocation, and it is
# what `run_guarded.sh` has been using all along.
#
# The limit is per *process* and is inherited by children, so a shell that spawns
# children gives each of them the same ceiling. The total is therefore bounded by
# (processes × ceiling) — which is why the cap should be paired with a bounded
# number of processes (`.cargo/config.toml` sets `jobs = 2` for exactly that
# reason).
#
# Usage: tools/memcap.sh <gib> <command> [args...]
#
#   tools/memcap.sh 1  ls -la
#   tools/memcap.sh 8  cargo build --release
#   tools/memcap.sh 8  cargo test --release
#   tools/memcap.sh 10 ./target/release/zentropy eval ...
#
# Recommended: 1 for trivial commands, 8 for anything that compiles, 10 for
# anything that codes.
set -eu

GIB="${1:?usage: memcap.sh <gib> <command> [args...]}"
shift
[ "$#" -ge 1 ] || { echo "memcap: need a command" >&2; exit 2; }

case "$GIB" in
    ''|*[!0-9]*) echo "memcap: cap must be a positive integer GiB, got '$GIB'" >&2; exit 2 ;;
esac
[ "$GIB" -ge 1 ] || { echo "memcap: cap must be at least 1 GiB" >&2; exit 2; }

KB=$((GIB * 1024 * 1024))
if ! ulimit -v "$KB" 2>/dev/null; then
    echo "memcap: FATAL: this shell cannot set RLIMIT_AS; refusing to run uncapped" >&2
    exit 3
fi

echo "memcap: RLIMIT_AS ${GIB} GiB on this command and everything it starts" >&2
exec "$@"
