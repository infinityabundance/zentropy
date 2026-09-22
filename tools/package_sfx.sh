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
echo "building submission stub (nightly + build-std, panic=immediate-abort)..." >&2
# THREE things are pinned here, and each was measured rather than assumed.
#
# 1. Toolchain. `nightly-2026-07-24` is pinned by date, not `+nightly`, so the
#    artifact is reproducible: an unpinned channel would silently move.
#
# 2. `-Z build-std=std,panic_abort` + `-Cpanic=immediate-abort`. This is the single
#    largest Phase-11 win by a wide margin: it rebuilds `std` without the panic
#    hook, the backtrace machinery and the unwinding tables that the *prebuilt*
#    std carries unconditionally (its prebuilt rlibs are compiled panic=unwind).
#    Measured saving: 400,816 B -> 111,888 B, i.e. **-288,928 B (-72%)**, and the
#    resulting stub produces a BYTE-IDENTICAL archive to the stable build
#    (verified on enwik6: 267,333 B both ways, exact round-trip). The bytes being
#    removed are `gimli`, `addr2line`, `miniz_oxide`, `rustc_demangle` and four
#    `quicksort` instantiations - none of which touch the codec.
#    A stable build still works and is 288,928 B larger; that is the fallback.
#
# 3. Target, named explicitly, so a host-specific default cannot leak in.
#
# Portability guard: RUSTFLAGS is set here rather than inherited, so a developer
# experimenting with `-C target-cpu=native` (see tools/build_research.sh) cannot
# leak a host-specific build into a scored artifact - a native build emits
# AVX2/BMI2 unconditionally, and the judged machines "may change without notice".
# --- target: static musl, and the reason is eligibility, not size -------------
#
# The dynamic glibc build is 105,536 B; this static-pie musl build is 125,056 B,
# i.e. +19,520 B per copy (+39,040 B of S, since both legal forms charge the
# program twice). That is 3.6% of the 1% gate, and it buys the difference between
# a submission that runs and one that might not: the dynamic build's symbol
# versions require **glibc >= 2.34** (Ubuntu 22.04+, Debian 12+), while the
# rules' Linux test machine dates from 2021 and the rules warn the machines "may
# change without notice". A binary that will not start scores nothing, so the
# static build is the primary artefact and the glibc one is the fallback:
#
#     ZENTROPY_TARGET=x86_64-unknown-linux-gnu sh tools/package_sfx.sh <in>
#
# Both are proven to produce BYTE-IDENTICAL archives (enwik6 246,808; enwik7
# 2,226,353), which is the condition for the swap to be legitimate at all — a
# different libc is exactly the kind of change that can move a result.
#
# The static build also removes the last external dependency: `ldd` reports
# "statically linked", so the judged program needs no shared library, no
# loader beyond what the kernel provides, and no glibc version at all.
TARGET_TRIPLE="${ZENTROPY_TARGET:-x86_64-unknown-linux-musl}"

# `VAR=value cmd` on one line, not on its own line: a bare assignment is a shell
# variable and is *not* exported to the child, which would silently build the
# stub without the panic-immediate-abort flags and give back the 288,928 bytes
# Phase 11 removed.
RUSTFLAGS="-Zunstable-options -Cpanic=immediate-abort" cargo +nightly-2026-07-24 \
    -Z build-std=std,panic_abort \
    build --quiet --profile submission --no-default-features \
    --features accepted,submission \
    --bin zentropy-sfx --target "$TARGET_TRIPLE"
STUB="$ROOT/target/$TARGET_TRIPLE/submission/zentropy-sfx"
BHM="$OUTDIR/$NAME.bhm"
SFX="$OUTDIR/${NAME}.archive9"

echo "compressing with the scored stub (comp9a == decomp9)..." >&2

# Hard address-space ceiling on the stub, not just its own `mem-guard` startup
# projection. The stub's guard is an estimate; `RLIMIT_AS` is not. The cap sits
# well above the measured peak (5.96 GiB at enwik9) and far below the machine, so
# a runaway allocates into a clean abort instead of into the rest of the session.
# `run_guarded.sh` also takes the single-heavy-run lock; sharded callers opt out
# with ZENTROPY_ALLOW_CONCURRENT=1, which is set by gate_many.sh deliberately.
CAP_GIB="${ZENTROPY_STUB_CAP_GIB:-9}"
guard() { sh "$ROOT/tools/run_guarded.sh" "$CAP_GIB" "$@"; }

guard "$STUB" c "$IN" "$BHM"

echo "verifying exactness with the same stub..." >&2
guard "$STUB" d "$BHM" "$OUTDIR/$NAME.decoded"
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
