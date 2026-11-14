#!/usr/bin/env bash
# Gate for one crate (`tools/check.sh crypto`) or the whole workspace (no argument).
# PASS only if format, lints, tests and the zero-dependency rule all hold.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -n "${1:-}" ]; then
  SEL="-p $1"; FMT="-p $1"
  export CARGO_TARGET_DIR="target/w-$1"   # own build dir so parallel workers do not block each other
else
  SEL="--workspace"; FMT="--all"
fi
cargo fmt $FMT -- --check
cargo clippy $SEL --all-targets -- -D warnings
cargo test $SEL -q
if [ -f Cargo.lock ] && grep -q '^source = ' Cargo.lock; then
  echo "FAIL: external dependency found in Cargo.lock (Strata is zero-dependency)"; exit 1
fi
echo "CHECK PASS ${1:-workspace}"
