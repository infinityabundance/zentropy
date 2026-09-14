#!/bin/sh
# Phase 9 gate: decide a searched `tune` on the authority corpus.
#
# The search layer only *screens*. Adoption is decided by a complete `eval` on
# the authority corpus that re-encodes both the parent and the candidate and
# reconstructs both exactly, so `DeltaS` is measured rather than projected.
#
# The tune knob itself costs zero executable bytes (the byte is already in the
# archive header and the rate table is charged once, at the phase level), so the
# gate is invoked with `--binary-cost 0`. Anything else would double-charge the
# phase mechanism per candidate.
#
# Usage: tools/p9_gate.sh <corpus> <candidate-tune> [parent-tune] [outdir]
#
# Long run: an enwik9 gate is ~2 h of encode+decode. Launch it detached
# (`nohup sh tools/p9_gate.sh ... &`) after the binary is already built; never
# chain it to a build, or the shell may signal the gate before it starts.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
CORPUS="${1:?usage: p9_gate.sh <corpus> <candidate-tune> [parent-tune] [outdir]}"
TUNE="${2:?usage: p9_gate.sh <corpus> <candidate-tune> [parent-tune] [outdir]}"
PARENT_TUNE="${3:-7}"
OUTDIR="${4:-$ROOT/evidence/runs/p9_enwik9}"

mkdir -p "$OUTDIR"
cd "$ROOT"

Z="$ROOT/target/release/zentropy"
if [ ! -x "$Z" ]; then
    echo "gate: building the release driver..." >&2
    cargo build --quiet --release
fi

# OOM protection: the gate runs under a hard address-space ceiling and a
# single-heavy-run lock (tools/run_guarded.sh), and the matching --max-ram keeps
# the driver's own startup budget in agreement with that ceiling.
CAP_GIB="${ZENTROPY_GATE_CAP_GIB:-8}"

echo "gate: corpus=$CORPUS candidate_tune=$TUNE parent_tune=$PARENT_TUNE" >&2
echo "gate: receipt=$OUTDIR/tune$TUNE.jsonl cap=${CAP_GIB}GiB" >&2

"$ROOT/tools/run_guarded.sh" "$CAP_GIB" "$Z" eval "$CORPUS" \
    --candidate residual --tune "$TUNE" \
    --parent residual --parent-tune "$PARENT_TUNE" \
    --binary-cost 0 --max-ram "${CAP_GIB}G" \
    --receipt "$OUTDIR/tune$TUNE.jsonl"
