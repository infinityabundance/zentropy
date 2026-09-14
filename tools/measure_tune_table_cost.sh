#!/bin/sh
# A31 — the measured marginal executable cost of the T2 table-size scale.
#
# The scored set gained exactly one mechanism in Phase 11's T2 work: the `tune`
# high nibble as a table-size scale. `S` is authority, so its price is measured
# by building the two otherwise-identical stubs the rule requires:
#
#   A  `accepted`                               — the shipped configuration
#   B  `accepted-core,pre-t2-geometry`          — the same set, nibble inert
#
# Both are built with the *pinned* toolchain, target and `build-std` flags from
# `tools/package_sfx.sh`, because at `opt-level="z"` artifact size is a layout
# property: comparing a nightly build-std stub against a stable stub would
# measure the toolchain, not the mechanism.
#
# Reported delta is the per-copy price. Both legal packaging forms charge the
# program **twice** (`S = 2P + bhm`; the self-extracting form embeds the stub in
# `archive9`), so the delta's effect on `S` is double the number printed.
#
# Usage: tools/measure_tune_table_cost.sh
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

RUSTFLAGS="-Zunstable-options -Cpanic=immediate-abort"
export RUSTFLAGS
TARGET_TRIPLE=x86_64-unknown-linux-gnu

build() {
    _features="$1"
    _out="$2"
    cargo +nightly-2026-07-24 -Z build-std=std,panic_abort \
        build --quiet --profile submission --no-default-features \
        --features "$_features" --bin zentropy-sfx --target "$TARGET_TRIPLE"
    cp "$ROOT/target/$TARGET_TRIPLE/submission/zentropy-sfx" "$_out"
}

echo "measuring marginal executable cost of: tune-table" >&2
echo "  A (accepted)                  = accepted" >&2
echo "  B (same set, nibble inert)    = accepted-core,pre-t2-geometry" >&2

echo "  building A ..." >&2
build "accepted" "$TMP/a"
echo "  building B ..." >&2
build "accepted-core,pre-t2-geometry" "$TMP/b"
# A31 asks for stability, not a single sample: a +/- 16 B layout wobble must not
# be reported as a mechanism price.
echo "  rebuilding B to check stability ..." >&2
build "accepted-core,pre-t2-geometry" "$TMP/b2"

A=$(wc -c < "$TMP/a")
B=$(wc -c < "$TMP/b")
B2=$(wc -c < "$TMP/b2")

echo ""
echo "stub_accepted_bytes=$A"
echo "stub_without_tune_table_bytes=$B"
echo "stub_without_tune_table_bytes_rebuild=$B2"
echo "measurement_stable=$([ "$B" = "$B2" ] && echo yes || echo no)"
echo "measured_marginal_binary_cost_bytes=$((A - B))"
echo "effect_on_S_bytes=$((2 * (A - B)))"
