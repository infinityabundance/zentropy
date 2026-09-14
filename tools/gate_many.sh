#!/bin/sh
# Gate several candidate tunes on one corpus, concurrently.
#
# Two facts drive this script's shape:
#
#   1. A single `eval` pass is inherently sequential — the predictor is a
#      context-mixing model whose state depends on every preceding bit, so one
#      encode cannot be spread across cores. The available parallelism is
#      *across candidates*, not within one.
#   2. Four passes per gate is two passes of waste. The parent is the accepted
#      configuration and its archive on this corpus is already receipted, so
#      `--parent-archive-bytes` skips re-encoding and re-decoding it. A gate is
#      then encode-candidate + decode-candidate, and the *candidate* is still
#      fully measured and proven exact in this run.
#
# Together: N candidates cost N encodes + N decodes, run JOBS at a time, instead
# of N × four passes run one at a time.
#
# Usage: tools/gate_many.sh <corpus> <parent-tune> <parent-archive-bytes> <tune>[,<tune>...]
#
# Environment:
#   ZENTROPY_GATE_JOBS      concurrent gates (default 3)
#   ZENTROPY_GATE_CAP_GIB   hard address-space cap per gate (default 8)
#   ZENTROPY_GATE_OUT       receipt directory (default evidence/runs/gates)
#   ZENTROPY_GATE_METHOD    method under test (default: the accepted method)
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
CORPUS="${1:?usage: gate_many.sh <corpus> <parent-tune> <parent-archive-bytes> <tune,...>}"
PTUNE="${2:?usage: gate_many.sh <corpus> <parent-tune> <parent-archive-bytes> <tune,...>}"
PBYTES="${3:?usage: gate_many.sh <corpus> <parent-tune> <parent-archive-bytes> <tune,...>}"
TUNES="${4:?usage: gate_many.sh <corpus> <parent-tune> <parent-archive-bytes> <tune,...>}"

JOBS="${ZENTROPY_GATE_JOBS:-3}"
CAP="${ZENTROPY_GATE_CAP_GIB:-8}"
OUTDIR="${ZENTROPY_GATE_OUT:-$ROOT/evidence/runs/gates}"
# The method under test; `residual` is the accepted configuration.
METHOD="${ZENTROPY_GATE_METHOD:-residual}"
mkdir -p "$OUTDIR"

Z="$ROOT/target/release/zentropy"
[ -x "$Z" ] || { echo "gate_many: building the release driver..." >&2; cargo build --quiet --release; }

# Normalise the tune list (commas or spaces both work).
LIST=$(printf '%s' "$TUNES" | tr ',' ' ')

launch() {
    _t="$1"
    echo "gate_many: launching tune=$_t (receipt $OUTDIR/tune$_t.jsonl)" >&2
    # Each gate is independent; the per-gate cap bounds it, so concurrency is
    # safe here by construction (JOBS * CAP must fit the machine).
    ZENTROPY_ALLOW_CONCURRENT=1 sh "$ROOT/tools/run_guarded.sh" "$CAP" "$Z" eval "$CORPUS" \
        --candidate "$METHOD" \
        --tune "$_t" \
        --parent-tune "$PTUNE" \
        --binary-cost 0 \
        --parent-archive-bytes "$PBYTES" \
        --max-ram "${CAP}G" \
        --receipt "$OUTDIR/tune$_t.jsonl" \
        > "$OUTDIR/tune$_t.log" 2>&1 &
}

remaining="$LIST"
while [ -n "$(printf '%s' "$remaining" | tr -d ' ')" ]; do
    n=0
    for t in $remaining; do
        [ "$n" -ge "$JOBS" ] && break
        launch "$t"
        n=$((n + 1))
    done
    wait
    # Drop the first n entries from the remaining list.
    i=0
    rest=""
    for t in $remaining; do
        i=$((i + 1))
        [ "$i" -le "$n" ] || rest="$rest $t"
    done
    remaining="$rest"
done

echo "gate_many: done; results:" >&2
for t in $LIST; do
    if [ -f "$OUTDIR/tune$t.jsonl" ]; then
        dec=$(sed -n 's/.*"decision":"\([^"]*\)".*/\1/p' "$OUTDIR/tune$t.jsonl" | tail -1)
        dl=$(sed -n 's/.*"archive_delta=\(-\{0,1\}[0-9]*\).*/\1/p' "$OUTDIR/tune$t.jsonl" | tail -1)
        printf '  tune %-4s %-10s delta=%s\n' "$t" "$dec" "$dl" >&2
    else
        printf '  tune %-4s (no receipt; see %s/tune%s.log)\n' "$t" "$OUTDIR" "$t" >&2
    fi
done
