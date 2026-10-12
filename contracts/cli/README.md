# Approved finite CLI baseline

Maintainer approval recorded on 2026-10-12 for the exact
[help/version slice](../../docs/proposals/finite-cli-activation-v1.md).
Only a single `--help` or `--version` is accepted (exit 0, empty stderr).
No arguments produce `no-arguments.stderr` (exit 2, empty stdout).
All other sequences produce `invalid-invocation.stderr` (exit 2, empty stdout),
including non-Unicode arguments. Files contain exact UTF-8 bytes and final newlines.
Version uses Cargo package version; intentional package version changes must update
its snapshot with a compatibility/version decision. No aliases or monitoring commands
are approved. Config, state, environment resources and providers are not loaded.
Installed-binary acceptance compares bytes, streams and exits without regenerating
these snapshots. Broader draft contracts remain unapproved.
