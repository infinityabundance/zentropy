#!/bin/sh
# Measure the true executable-byte cost of a mechanism.
#
# `S` is authority, so a mechanism's binary cost must be measured, not guessed.
# This builds two otherwise-identical submission binaries -- one with the
# mechanism compiled in, one with it feature-disabled -- and reports the delta.
#
# Usage: tools/measure_binary_cost.sh [feature1,feature2,...]
#        (default feature: struct-hoist)
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
FEATURES="${1:-struct-hoist}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

cd "$ROOT"

echo "measuring executable cost of feature(s): $FEATURES" >&2
echo "  building WITH  ..." >&2
cargo build --quiet --profile submission --bin zentropy-sfx
cp "$ROOT/target/submission/zentropy-sfx" "$TMP/with"

echo "  building WITHOUT (--no-default-features) ..." >&2
cargo build --quiet --profile submission --no-default-features --bin zentropy-sfx
cp "$ROOT/target/submission/zentropy-sfx" "$TMP/without"

WITH=$(wc -c < "$TMP/with")
WITHOUT=$(wc -c < "$TMP/without")
DELTA=$((WITH - WITHOUT))

echo "binary_with_bytes=$WITH"
echo "binary_without_bytes=$WITHOUT"
echo "measured_binary_cost_bytes=$DELTA"
echo ""

# Leave the tree holding the default (mechanism-enabled) submission stub so
# subsequent packaging does not silently use the measurement build.
cargo build --quiet --profile submission --bin zentropy-sfx

echo "Pass this to the experiment, e.g.:"
echo "  zentropy hoist <corpus> --binary-cost $DELTA --receipt evidence/runs/receipts.jsonl"
