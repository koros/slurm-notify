#!/bin/sh
# Shared local/CI checks. Run from the repository root.
set -eu
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s tests/integration/storage -p 'test_*.py' -v
python3 -m unittest discover -s tests/integration/hooks -p 'test_*.py' -v
cargo build --workspace --locked
set +e
output=$("${CARGO_TARGET_DIR:-target}/debug/slurm-notify" 2>&1)
status=$?
set -e
test "$status" -eq 1
test "$output" = "Slurm Notify is under development; monitoring is not implemented."
