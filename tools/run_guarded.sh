#!/bin/sh
# Run a memory-heavy command with the outer two layers of OOM protection.
#
# `src/memory.rs` provides the inner layers: a startup budget that refuses a run
# that cannot fit (with a 4 GiB reserve kept for the workstation), and a runtime
# floor that aborts a long run if available memory falls while it codes. This
# wrapper adds the two things a process cannot enforce on itself:
#
#   1. **A hard kernel ceiling** (`ulimit -v`, i.e. RLIMIT_AS). The startup
#      projection is conservative but it is still an estimate; if a run outgrows
#      it, the process must die rather than drive the whole machine into swap
#      until the kernel picks some *other* process — very likely the user's
#      editor — as the OOM victim.
#   2. **Serialisation.** Two heavy runs on one workstation is how a machine
#      thrashes while every individual budget looks fine. One heavy run at a
#      time is the default; deliberately sharded campaigns opt out with
#      `ZENTROPY_ALLOW_CONCURRENT=1`.
#
# Usage: tools/run_guarded.sh <hard-cap-gib> <cmd> [args...]
#
# The cap should sit comfortably above the driver's own projection
# (`zentropy meminfo <corpus>`) to leave address-space slack, and the caller
# should pass a matching `--max-ram` so the two agree.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
GIB="${1:?usage: run_guarded.sh <hard-cap-gib> <cmd> [args...]}"
shift
[ "$#" -ge 1 ] || { echo "run_guarded: need a command" >&2; exit 2; }
# `ulimit -v 0` is not a tight limit on every shell, so refuse it rather than
# silently running unbounded.
case "$GIB" in
    ''|*[!0-9]*) echo "run_guarded: cap must be a positive integer GiB, got '$GIB'" >&2; exit 2 ;;
esac
[ "$GIB" -ge 1 ] || { echo "run_guarded: cap must be at least 1 GiB" >&2; exit 2; }

# --- 2. serialisation -------------------------------------------------------
LOCK="${TMPDIR:-/tmp}/zentropy-heavy.lock"
HELD=0
if [ "${ZENTROPY_ALLOW_CONCURRENT:-0}" != "1" ]; then
    if mkdir "$LOCK" 2>/dev/null; then
        HELD=1
    else
        pid=$(cat "$LOCK/pid" 2>/dev/null || true)
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            echo "run_guarded: refusing: heavy run pid $pid is already in flight." >&2
            echo "  Wait for it, or set ZENTROPY_ALLOW_CONCURRENT=1 for deliberate sharding." >&2
            exit 75
        fi
        # Stale lock from a crashed run: take it over.
        rm -rf "$LOCK"
        if mkdir "$LOCK" 2>/dev/null; then
            HELD=1
        fi
    fi
    [ "$HELD" -eq 1 ] && echo "$$" > "$LOCK/pid"
fi
release() {
    [ "$HELD" -eq 1 ] && rm -rf "$LOCK"
    return 0
}
trap 'release' EXIT INT TERM

# --- 1. hard ceiling --------------------------------------------------------
KB=$((GIB * 1024 * 1024))
if ulimit -v "$KB" 2>/dev/null; then
    echo "run_guarded: hard address-space cap ${GIB} GiB (RLIMIT_AS)" >&2
else
    echo "run_guarded: warning: ulimit -v unsupported here; relying on the" >&2
    echo "             startup budget and runtime floor alone" >&2
fi

# --- run --------------------------------------------------------------------
# Not `exec`: the lock must be released when the command finishes.
status=0
"$@" || status=$?
release
trap - EXIT INT TERM
exit "$status"
