#!/bin/sh
# Shared local/CI checks. Run from the repository root.
set -eu
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s tests/integration/storage -p 'test_*.py' -v
python3 -m unittest discover -s tests/integration/hooks -p 'test_*.py' -v
python3 -m unittest discover -s tests/integration/config_drafts -p 'test_*.py' -v
cargo build --workspace --locked
set +e
output=$("${CARGO_TARGET_DIR:-target}/debug/slurm-notify" 2>&1)
status=$?
set -e
test "$status" -eq 2
test "$output" = "Usage: slurm-notify --help | --version"
mkdir -p target
"${CARGO_TARGET_DIR:-target}/debug/slurm-notify" --help > target/cli-help.stdout
cmp contracts/cli/help.stdout target/cli-help.stdout
"${CARGO_TARGET_DIR:-target}/debug/slurm-notify" --version > target/cli-version.stdout
cmp contracts/cli/version.stdout target/cli-version.stdout
