#!/bin/sh
# Phase 7 enwik9 authority gate: the category-first page order on `sse-3`.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
BIN=./target/release/zentropy
R=evidence/runs/p7_enwik9
mkdir -p "$R"
"$BIN" eval evidence/corpus/enwik9 --candidate reorder-category --parent sse-3 --binary-cost 0 --receipt "$R/category.jsonl" > "$R/category.log" 2>&1
echo "phase7 category enwik9 gate complete"
cat "$R/category.log"
