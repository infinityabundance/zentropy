#!/bin/sh
# Baseline compression harness.
#
# Measures the conventional compressors on a corpus slice and records, for each,
# the compressed bytes, wall time, and (where meaningful) the decompressor size.
# The Hutter score is only meaningful for self-contained submissions; for the
# generic tools we record a "Hutter-equivalent" that charges the program bytes,
# because comparing bare ratios is exactly the mistake the constitution forbids.
#
# Usage: tools/baseline.sh <input> <output-jsonl>
set -eu

IN="$1"
OUT="$2"
NAME=$(basename "$IN")
BYTES=$(wc -c < "$IN")
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

emit() {
    # $1 = program label, $2 = compressed bytes, $3 = program bytes, $4 = seconds, $5 = note
    total=$(( $2 + $3 ))
    printf '{"corpus":"%s","input_bytes":%s,"program":"%s","archive_bytes":%s,"program_bytes":%s,"hutter_equivalent":%s,"seconds":%s,"note":"%s"}\n' \
        "$NAME" "$BYTES" "$1" "$2" "$3" "$total" "$4" "$5" >> "$OUT"
    printf '%-22s archive=%12s prog=%8s total=%12s %ss\n' "$1" "$2" "$3" "$total" "$4"
}

now() { date +%s.%N; }
elapsed() { awk "BEGIN{printf \"%.3f\", $2-$1}"; }

: > "$OUT"

# gzip -9
if command -v gzip >/dev/null 2>&1; then
    t0=$(now); gzip -9 -c "$IN" > "$TMP/gz"; t1=$(now)
    emit "gzip -9" "$(wc -c < "$TMP/gz")" "$(wc -c < "$(command -v gzip)")" "$(elapsed "$t0" "$t1")" "LZ77"
fi

# bzip2 -9
if command -v bzip2 >/dev/null 2>&1; then
    t0=$(now); bzip2 -9 -c "$IN" > "$TMP/bz2"; t1=$(now)
    emit "bzip2 -9" "$(wc -c < "$TMP/bz2")" "$(wc -c < "$(command -v bzip2)")" "$(elapsed "$t0" "$t1")" "BWT"
fi

# zstd
if command -v zstd >/dev/null 2>&1; then
    t0=$(now); zstd -19 --ultra -22 -q -c "$IN" > "$TMP/zst"; t1=$(now)
    emit "zstd -19 --ultra -22" "$(wc -c < "$TMP/zst")" "$(wc -c < "$(command -v zstd)")" "$(elapsed "$t0" "$t1")" "LZ77+FSE"
fi

# xz
if command -v xz >/dev/null 2>&1; then
    t0=$(now); xz -9e -c "$IN" > "$TMP/xz"; t1=$(now)
    emit "xz -9e" "$(wc -c < "$TMP/xz")" "$(wc -c < "$(command -v xz)")" "$(elapsed "$t0" "$t1")" "LZMA2"
fi

# brotli
if command -v brotli >/dev/null 2>&1; then
    t0=$(now); brotli -q 11 -c "$IN" > "$TMP/br"; t1=$(now)
    emit "brotli -q 11" "$(wc -c < "$TMP/br")" "$(wc -c < "$(command -v brotli)")" "$(elapsed "$t0" "$t1")" "LZ77+dict"
fi

printf '\nRecorded to %s\n' "$OUT"
