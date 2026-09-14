#!/bin/sh
# Build the *research* driver with host-specific codegen (BMI2/AVX2 etc.).
#
# Measured: ~13% faster than the portable build on enwik7 (30.0 s -> 26.1 s under
# identical load), which matters because a single enwik9 pass is ~33 min.
#
# This must NEVER be used for a scored artifact. `-C target-cpu=native` emits
# AVX2/BMI2 unconditionally, the x86-64 baseline does not include them, and the
# Hutter rules say the test machines "may change without notice" — a judge
# machine without AVX2 would fail to run the binary at all. The submission is
# packaged by `tools/package_sfx.sh`, which builds with the portable toolchain.
#
# A side effect worth knowing: RUSTFLAGS changes Cargo's fingerprint, so
# switching between this and a plain `cargo build --release` rebuilds the crate.
#
# Usage: tools/build_research.sh [extra cargo args]
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

echo "build_research: -C target-cpu=native (research only; NOT for a submission)" >&2
RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=native" cargo build --release "$@"
