#!/bin/sh
# Rust workspace line coverage. Requires cargo-llvm-cov 0.6.21 and llvm-tools-preview.
set -eu
mkdir -p target/coverage
export CARGO_LLVM_COV_TARGET_DIR="$PWD/target/llvm-cov-target"
# The supervisor deliberately clears LLVM_PROFILE_FILE in children. Remove only
# stale fallback profiles before this run, then collect successful child exits.
python3 - <<'PY_START'
from pathlib import Path
for profile in Path("crates/notify-cli").glob("default_*.profraw"):
    profile.unlink()
PY_START
cargo llvm-cov --workspace --locked --no-report
python3 - <<'PY_COLLECT'
from pathlib import Path
for profile in Path("crates/notify-cli").glob("default_*.profraw"):
    profile.rename(Path("target/llvm-cov-target") / profile.name)
PY_COLLECT
# Write reports before enforcing the threshold so failed gates remain inspectable.
cargo llvm-cov report --json --output-path target/coverage/rust.json
cargo llvm-cov report --lcov --output-path target/coverage/lcov.info
cargo llvm-cov report --html --output-dir target/coverage
cargo llvm-cov report --fail-under-lines 80
