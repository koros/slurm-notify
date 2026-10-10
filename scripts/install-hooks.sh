#!/bin/sh
set -eu
root=$(git rev-parse --show-toplevel)
cd "$root"
existing=$(git config --get core.hooksPath || true)
if [ -n "$existing" ] && [ "$existing" != ".githooks" ]; then
    printf 'Existing core.hooksPath is %s; preserve or migrate those hooks before installing.\n' "$existing" >&2
    exit 1
fi
for program in git cargo rustup python3; do
    command -v "$program" >/dev/null 2>&1 || { printf 'Required command is missing: %s\n' "$program" >&2; exit 1; }
done
git config --local core.hooksPath .githooks
printf 'Installed repository-local pre-commit and pre-push hooks.\n'
