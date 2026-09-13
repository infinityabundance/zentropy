#!/bin/sh
# Phase 6 enwik9 authority gate (corrected): PPM-C on top of the accepted
# parent `sse-3` (phase4 + extra SSE stage), and its control.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
BIN=./target/release/zentropy
R=evidence/runs/p6_enwik9
mkdir -p "$R"

"$BIN" eval evidence/corpus/enwik9 --candidate ppm     --parent sse-3 --binary-cost 0 --receipt "$R/ppm2.jsonl"     > "$R/ppm2.log" 2>&1 &
P1=$!
"$BIN" eval evidence/corpus/enwik9 --candidate ppm-ctl --parent sse-3 --binary-cost 0 --receipt "$R/ppm_ctl2.jsonl" > "$R/ppm_ctl2.log" 2>&1 &
P2=$!

wait $P1 $P2
echo "phase6 enwik9 ppm gates complete"
cat "$R/ppm2.log" "$R/ppm_ctl2.log"
