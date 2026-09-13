#!/bin/sh
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
BIN=./target/release/zentropy
R=evidence/runs/p7_enwik9
mkdir -p "$R"
"$BIN" eval evidence/corpus/enwik9 --candidate reorder-full --parent sse-3 --binary-cost 0 --receipt "$R/full.jsonl" > "$R/full.log" 2>&1
echo "phase7 full enwik9 gate complete"
cat "$R/full.log"
