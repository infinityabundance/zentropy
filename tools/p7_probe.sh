#!/bin/sh
# Phase 7 probe: establish the page structure and the free-restoration
# precondition of a corpus, and quantify the permutation cost the id-sort avoids.
#
# Usage: tools/p7_probe.sh [corpus]
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
CORPUS="${1:-$ROOT/evidence/corpus/enwik9}"
Z="$ROOT/target/release/zentropy"

if [ ! -x "$Z" ]; then
    echo "building the driver (with the reorder feature) ..." >&2
    (cd "$ROOT" && cargo build --quiet --release --features reorder)
fi

echo "=== zentropy Phase 7 page-structure probe ==="
"$Z" reorder-info "$CORPUS"
echo ""
echo "=== interpretation ==="
echo "pages: number of complete <page> blocks."
echo "page_id_strictly_ascending: if true, the original order is restored by a"
echo "  stable sort on the embedded page id at zero archive-byte cost; if false,"
echo "  the encoder falls back to the parent method and never reorders."
echo "explicit_permutation_bytes: ceil(log2(pages!))/8, what a stored permutation"
echo "  would cost; the id-sort pays 0."
