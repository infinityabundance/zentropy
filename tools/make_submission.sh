#!/bin/sh
# Phase 12: produce the complete submission bundle in one command.
#
# Everything a judge needs, in one directory, with the score measured rather than
# asserted:
#
#   comp9a            the program (== decomp9; the two are the same file)
#   archive9.bhm      the compressed corpus
#   archive9          the self-extracting form (program ‖ marker ‖ len ‖ archive)
#   source.tar.gz     the source tree, dependency-free, for the source-submission form
#   MANIFEST.txt      sizes, digests, `S` for both legal forms, and how it was built
#
# The bundle is only written after the shipped stub has reconstructed the corpus
# byte-for-byte using only submitted bytes, so a broken bundle cannot be shipped.
#
# Usage: tools/make_submission.sh <corpus> [outdir]
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
CORPUS_ARG="${1:?usage: make_submission.sh <corpus> [outdir]}"
# Absolute, because the self-containment check below runs from a scratch
# directory. A relative corpus path silently makes that check compare against a
# missing file and fail for the wrong reason.
case "$CORPUS_ARG" in
    /*) CORPUS="$CORPUS_ARG" ;;
    *) CORPUS="$(cd "$(dirname "$CORPUS_ARG")" && pwd)/$(basename "$CORPUS_ARG")" ;;
esac
OUT="${2:-$ROOT/submission}"
NAME=$(basename "$CORPUS")

cd "$ROOT"
rm -rf "$OUT"
mkdir -p "$OUT"

echo "=== 12.1/12.2  building the scored stub and packing both forms ===" >&2
# Reuses the packaging path, so the toolchain, target, flags and accounting are
# the ones documented in docs/SUBMISSION_CHECKLIST.md — not a second copy that
# can drift.
PACK=$(sh "$ROOT/tools/package_sfx.sh" "$CORPUS" "$OUT" 2>&1)
printf '%s\n' "$PACK" >&2

TARGET_TRIPLE="${ZENTROPY_TARGET:-x86_64-unknown-linux-musl}"
STUB="$ROOT/target/$TARGET_TRIPLE/submission/zentropy-sfx"
BHM="$OUT/$NAME.bhm"
SFX="$OUT/$NAME.archive9"

# The two legal forms. `comp9a == decomp9`, so the separate form charges the
# single program twice: S = 2P + bhm.
P=$(wc -c < "$STUB")
B=$(wc -c < "$BHM")
A=$(wc -c < "$SFX")

echo "=== 12.2  proving the shipped stub reconstructs from submitted bytes alone ===" >&2
# Deliberately *not* the driver: the artefact under test is the stub.
"$STUB" d "$BHM" "$OUT/.verify"
cmp -s "$CORPUS" "$OUT/.verify" || {
    echo "FAIL: the shipped stub did not reconstruct the corpus byte-for-byte" >&2
    exit 1
}
rm -f "$OUT/.verify"

echo "=== 12.4  self-containment: empty directory, scrubbed environment ===" >&2
( cd "$OUT" && env -i "./$NAME.archive9" && cmp -s data9 "$CORPUS" ) || {
    echo "FAIL: the self-extracting form does not reconstruct under env -i" >&2
    exit 1
}
rm -f "$OUT/data9"

echo "=== 12.7  source form ===" >&2
# The scored path has no dependencies, so the archive needs no vendoring. Excluded:
# the research plane's data (corpus, targets, receipts) — they are large and none
# of them is needed to rebuild the submission.
( cd "$ROOT" && tar czf "$OUT/source.tar.gz" \
    --exclude=./target --exclude=./evidence/corpus --exclude=./.git \
    --exclude=./research --exclude=./submission \
    src Cargo.toml Cargo.lock .cargo tools docs LICENSE-MIT README.md 2>/dev/null )

echo "=== assembling the bundle ===" >&2
cp "$STUB" "$OUT/comp9a"
cp "$STUB" "$OUT/decomp9"
cp "$SFX" "$OUT/archive9"

{
    echo "Zentropy submission bundle"
    echo "corpus      = $NAME ($(wc -c < "$CORPUS") bytes)"
    echo "corpus_sha256 = $(sha256sum "$CORPUS" | cut -d' ' -f1)"
    echo "archive_sha256 = $(sha256sum "$BHM" | cut -d' ' -f1)"
    echo "revision    = $(git --no-pager -C "$ROOT" rev-parse HEAD 2>/dev/null || echo '<no git>')"
    echo "target      = $TARGET_TRIPLE"
    echo "features    = accepted,submission"
    echo "profile     = submission (opt-level=z, lto=fat, codegen-units=1, panic=abort, overflow-checks=on)"
    echo "toolchain   = nightly-2026-07-24, -Z build-std=std,panic_abort, -Cpanic=immediate-abort"
    echo ""
    echo "program_bytes     = $P   (comp9a == decomp9)"
    echo "archive_bhm_bytes = $B"
    echo "archive9_bytes    = $A"
    echo ""
    echo "S(self-extracting: comp9 + archive9)      = $((P + A))"
    echo "S(separate, comp9a=decomp9: 2P + bhm)     = $((2 * P + B))"
    echo ""
    echo "exactness = PASS (byte-identical, via the shipped stub and under env -i)"
    echo ""
    echo "files: comp9a decomp9 archive9.bhm archive9 source.tar.gz MANIFEST.txt"
} > "$OUT/MANIFEST.txt"

cat "$OUT/MANIFEST.txt"
echo "" >&2
echo "bundle written to $OUT" >&2
