#!/bin/sh
# Phase 14.34: hidden-width sweep for the temporal residual corrector.
#
# The mechanism is a post-mixer logit correction, so it adds capacity without
# adding a mixer input. Its model is tiny (a 16-unit net is 706 B), so the
# interesting question before the authority gate is whether a wider net buys
# materially more archive for a still-small weight bill.
#
# Usage: tools/p14_temporal_width_sweep.sh <corpus> <parent-archive-bytes> [hidden...]
# Each width: trains on the corpus's own transformed stream, rebuilds so the
# weights embed, then runs a real encode+decode gate.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
Z="$ROOT/target/release/zentropy"
CORPUS="${1:?usage: p14_temporal_width_sweep.sh <corpus> <parent-bytes> [hidden...]}"
PARENT="${2:?usage: p14_temporal_width_sweep.sh <corpus> <parent-bytes> [hidden...]}"
shift 2
HIDDENS="${*:-8 16 32 64 128}"

echo "width sweep on $CORPUS (parent $PARENT B)"
for H in $HIDDENS; do
    echo "--- hidden=$H ---"
    tools/memcap.sh 10 "$Z" train-temporal "$CORPUS" --hidden "$H" \
        --out "$ROOT/src/learned/temporal.bin" 2>&1 | tail -1
    (cd "$ROOT" && tools/memcap.sh 10 cargo build --quiet --release 2>&1 | tail -1)
    tools/memcap.sh 10 "$Z" eval "$CORPUS" --candidate ph14-temporal \
        --tune 51 --parent-tune 51 --binary-cost 0 \
        --parent-archive-bytes "$PARENT" 2>&1 \
        | grep -E "candidate |archive_delta|decision"
done
