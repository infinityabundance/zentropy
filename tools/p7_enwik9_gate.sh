#!/bin/sh
# Phase 7 enwik9 authority gates: the title and structural page orders on the
# accepted parent `sse-3`.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
BIN=./target/release/zentropy
R=evidence/runs/p7_enwik9
mkdir -p "$R"

"$BIN" eval evidence/corpus/enwik9 --candidate reorder-title  --parent sse-3 --binary-cost 0 --receipt "$R/title.jsonl"  > "$R/title.log" 2>&1 &
P1=$!
"$BIN" eval evidence/corpus/enwik9 --candidate reorder-struct --parent sse-3 --binary-cost 0 --receipt "$R/struct.jsonl" > "$R/struct.log" 2>&1 &
P2=$!

wait $P1 $P2
echo "phase7 enwik9 gates complete"
cat "$R/title.log" "$R/struct.log"
