#!/bin/sh
# A31 — Measure the true marginal executable-byte cost of one mechanism.
#
# `S` is authority, so a mechanism's binary cost must be measured, not guessed.
# Builds two otherwise-identical submission binaries:
#   * ALL                    -- every Phase-A mechanism compiled in
#   * ALL minus TARGET       -- the mechanism under test removed
# and reports the delta.
#
# Usage: tools/measure_binary_cost.sh [target-feature]
#        default target: struct-hoist
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET="${1:-struct-hoist}"
ALL="struct-hoist,alphabet-perm,info-inherit,column-model,case-model,word-token,word-token2,long-match,sparse-match,rep-state,match-byte,dist-match,stem,word-class,phrase,dict-front,affix-token,grammar,state-map,sse-3,isse,collision,ppm,reorder,learned"

# ALL minus TARGET (comma list).
REST=$(printf '%s' "$ALL" | tr ',' '\n' | grep -vx "$TARGET" | paste -sd, -)
if [ -z "$REST" ]; then
    echo "error: target '$TARGET' is the only feature; cannot form a baseline" >&2
    exit 2
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
cd "$ROOT"

echo "measuring marginal executable cost of: $TARGET" >&2
echo "  ALL              = $ALL" >&2
echo "  ALL minus target = $REST" >&2

echo "  building ALL ..." >&2
cargo build --quiet --profile submission --no-default-features --features "$ALL" --bin zentropy-sfx
cp "$ROOT/target/submission/zentropy-sfx" "$TMP/all"

echo "  building ALL-minus-$TARGET ..." >&2
cargo build --quiet --profile submission --no-default-features --features "$REST" --bin zentropy-sfx
cp "$ROOT/target/submission/zentropy-sfx" "$TMP/rest"

A=$(wc -c < "$TMP/all")
B=$(wc -c < "$TMP/rest")
DELTA=$((A - B))

echo "binary_ALL_bytes=$A"
echo "binary_without_${TARGET}_bytes=$B"
echo "measured_marginal_binary_cost_bytes=$DELTA"
echo ""

# Leave the default (all mechanisms) submission stub in place.
cargo build --quiet --profile submission --bin zentropy-sfx

echo "Pass this to a Phase-A evaluation, e.g.:"
echo "  zentropy eval <corpus> --candidate <method> --parent struct-hoist \\"
echo "      --binary-cost $DELTA --receipt evidence/runs/receipts.jsonl"
