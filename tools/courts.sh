#!/bin/sh
# The Zentropy court runner.
#
# Nothing becomes a claim without a court (§24). This script convenes the courts
# that can be automated today. Exit status is nonzero if any court fails.
#
# Usage: tools/courts.sh [corpus-dir]
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
CORPUS_DIR="${1:-$ROOT/evidence/corpus}"
Z="$ROOT/target/release/zentropy"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

pass=0
fail=0
court() {
    name="$1"; shift
    if "$@" >"$TMP/out" 2>&1; then
        printf 'PASS  %s\n' "$name"
        pass=$((pass + 1))
    else
        printf 'FAIL  %s\n' "$name"
        sed 's/^/      /' "$TMP/out"
        fail=$((fail + 1))
    fi
}

echo "=== Zentropy courts ==="
echo ""

# 1. Unit + property exactness courts (transform reversibility, coder round-trip,
#    IR round-trip on random and malformed input).
court "unit + property tests" cargo test --quiet --release

# 2. Determinism / cross-build: the same input must produce the same archive.
court "selftest end-to-end" "$Z" selftest

# 3. Transform reversibility court on real corpus slices.
if [ -f "$CORPUS_DIR/enwik6" ]; then
    court "IR exactness on enwik6" "$Z" tokenize "$CORPUS_DIR/enwik6"
fi

# 4. Reconstruction court: decode(encode(x)) == x on a corpus slice, with an
#    independent hash check.
if [ -f "$CORPUS_DIR/enwik6" ]; then
    "$Z" compress "$CORPUS_DIR/enwik6" "$TMP/enwik6.bhm" 2>/dev/null
    court "reconstruction court (enwik6)" "$Z" verify "$CORPUS_DIR/enwik6" "$TMP/enwik6.bhm"
fi

# 5. Decoder-corruption court: malformed archives must be rejected without
#    uncontrolled allocation or panic.
court "archive corruption court" "$Z" corrupt-court

# 6. Negative control: a synthetic incompressible stream must not compress.
court "negative control (incompressible)" "$Z" negative-court

# 7. Submission-packaging court: the self-extracting archive reconstructs.
if [ -f "$CORPUS_DIR/enwik6" ]; then
    court "submission packaging court" sh "$ROOT/tools/package_sfx.sh" "$CORPUS_DIR/enwik6" "$TMP"
fi

echo ""
echo "courts: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
