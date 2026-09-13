#!/bin/sh
# Phase 6 enwik9 authority gates: sse-3, state-map, and the sse-3 control.
# Run with plain `nohup sh tools/p6_enwik9_gate.sh` (not chained after a build).
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
BIN=./target/release/zentropy
R=evidence/runs/p6_enwik9
mkdir -p "$R"

"$BIN" eval evidence/corpus/enwik9 --candidate state-map  --parent phase4 --binary-cost 0 --receipt "$R/sm.jsonl"      > "$R/sm.log" 2>&1 &
P1=$!
"$BIN" eval evidence/corpus/enwik9 --candidate sse-3     --parent phase4 --binary-cost 0 --receipt "$R/sse.jsonl"     > "$R/sse.log" 2>&1 &
P2=$!
"$BIN" eval evidence/corpus/enwik9 --candidate sse-3-ctl --parent phase4 --binary-cost 0 --receipt "$R/sse_ctl.jsonl" > "$R/sse_ctl.log" 2>&1 &
P3=$!

wait $P1 $P2 $P3
echo "phase6 enwik9 gates complete"
cat "$R"/*.log
