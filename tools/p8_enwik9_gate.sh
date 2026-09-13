#!/bin/sh
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
BIN=./target/release/zentropy
R=evidence/runs/p8_enwik9
mkdir -p "$R"
"$BIN" eval evidence/corpus/enwik9 --candidate residual     --parent reorder-full --binary-cost 7848 --receipt "$R/residual.jsonl" > "$R/residual.log" 2>&1 &
P1=$!
"$BIN" eval evidence/corpus/enwik9 --candidate residual-ctl --parent reorder-full --binary-cost 7848 --receipt "$R/residual_ctl.jsonl" > "$R/residual_ctl.log" 2>&1 &
P2=$!
wait $P1 $P2
echo "phase8 enwik9 gates complete"
cat "$R/residual.log" "$R/residual_ctl.log"
