#!/bin/sh
# Build the submission-plane artefact and measure the real Hutter split.
#
# Produces, for a given input corpus:
#   * <out>.bhm   — the archive (decomp9 form)
#   * <out>       — a self-extracting archive9 (stub + payload)
# and prints S for both legal submission forms.
#
# Usage: tools/package_sfx.sh <input> [outdir]
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
IN="$1"
OUTDIR="${2:-$ROOT/evidence/runs}"
NAME=$(basename "$IN")
mkdir -p "$OUTDIR"

cd "$ROOT"
echo "building submission stub (profile=submission, features=accepted)..." >&2
# Portability guard: pin default codegen. A developer experimenting with
# `RUSTFLAGS=-C target-cpu=native` (see tools/build_research.sh) must not be able
# to leak a host-specific build into a scored artifact — a native build emits
# AVX2/BMI2 unconditionally, and the judged machines "may change without notice".
# Measured: native is ~13% faster for research; x86-64-v2 is a null result, so
# there is no portability-safe codegen win to claim here.
RUSTFLAGS="" cargo build --quiet --profile submission --no-default-features --features accepted --bin zentropy-sfx

STUB="$ROOT/target/submission/zentropy-sfx"
BHM="$OUTDIR/$NAME.bhm"
SFX="$OUTDIR/${NAME}.archive9"

echo "compressing with the scored stub (comp9a == decomp9)..." >&2
"$STUB" c "$IN" "$BHM"

echo "verifying exactness with the same stub..." >&2
"$STUB" d "$BHM" "$OUTDIR/$NAME.decoded"
if ! cmp -s "$IN" "$OUTDIR/$NAME.decoded"; then
    echo "FAIL: reconstruction is not byte-identical" >&2
    exit 1
fi
rm -f "$OUTDIR/$NAME.decoded"

"$ROOT/target/release/zentropy" pack-sfx "$STUB" "$BHM" "$SFX"

STUB_BYTES=$(wc -c < "$STUB")
BHM_BYTES=$(wc -c < "$BHM")
SFX_BYTES=$(wc -c < "$SFX")

echo ""
echo "corpus=$NAME input_bytes=$(wc -c < "$IN")"
echo "program_bytes=$STUB_BYTES"
echo "archive_bhm_bytes=$BHM_BYTES"
echo "archive9_bytes=$SFX_BYTES"
# Hutter score forms. The separate form charges the program twice: with
# comp9a == decomp9 the rule's 2x coefficient on decomp9 reduces to 1x, giving
# comp9a + decomp9 + bhm == 2*program + bhm.
echo "S(self-extracting: comp9 + archive9)  = $((STUB_BYTES + SFX_BYTES))"
echo "S(separate, comp9a=decomp9: 2P + bhm)  = $((2 * STUB_BYTES + BHM_BYTES))"
echo ""
echo "exactness: PASS (byte-identical)"
