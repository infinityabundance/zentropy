#!/bin/sh
# A0 — Freeze the Optimization-A parent.
#
# Binds the exact revision, toolchain, artifact bytes, corpus digests and current
# scores before any mechanism work. Optimization A may not use estimated binary
# cost anywhere; this file records *measured* artifact sizes only.
#
# Usage: tools/freeze_parent.sh
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
OUT="$ROOT/evidence/optimization-a"
mkdir -p "$OUT"

REV=$(git rev-parse HEAD 2>/dev/null || echo "<no-git>")
DIRTY=$(git --no-optional-locks status --porcelain 2>/dev/null | wc -l | tr -d ' ')
RUSTC=$(rustc -V 2>/dev/null || echo unknown)

echo "building frozen artifacts..." >&2
cargo build --quiet --release --bin zentropy
cargo build --quiet --profile submission --bin zentropy-sfx

DRIVER="$ROOT/target/release/zentropy"
STUB="$ROOT/target/submission/zentropy-sfx"

sha() { sha256sum "$1" | awk '{print $1}'; }
sz() { wc -c < "$1" | tr -d ' '; }

CORPUS_DIR="$ROOT/evidence/corpus"
corpus_hash() {
    if [ -f "$CORPUS_DIR/$1" ]; then sha "$CORPUS_DIR/$1"; else echo "<absent>"; fi
}
corpus_size() {
    if [ -f "$CORPUS_DIR/$1" ]; then sz "$CORPUS_DIR/$1"; else echo 0; fi
}

cat > "$OUT/PARENT.json" <<EOF
{
  "phase": "optimization-a",
  "role": "frozen-parent",
  "revision": "$REV",
  "dirty_files": $DIRTY,
  "toolchain": "$RUSTC",
  "profiles": { "driver": "release", "submission": "submission" },
  "artifacts": {
    "driver_bytes": $(sz "$DRIVER"),
    "driver_sha256": "$(sha "$DRIVER")",
    "submission_stub_bytes": $(sz "$STUB"),
    "submission_stub_sha256": "$(sha "$STUB")"
  },
  "corpus": {
    "enwik6": { "bytes": $(corpus_size enwik6), "sha256": "$(corpus_hash enwik6)" },
    "enwik7": { "bytes": $(corpus_size enwik7), "sha256": "$(corpus_hash enwik7)" },
    "enwik8": { "bytes": $(corpus_size enwik8), "sha256": "$(corpus_hash enwik8)" },
    "enwik9": { "bytes": $(corpus_size enwik9), "sha256": "$(corpus_hash enwik9)" }
  },
  "accounting": {
    "binary_cost_policy": "measured by tools/measure_binary_cost.sh; estimates forbidden",
    "score_forms": ["self_extracting", "separate", "shared_program"]
  }
}
EOF

echo "wrote $OUT/PARENT.json"
cat "$OUT/PARENT.json"
