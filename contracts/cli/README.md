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

## Configuration-validation addition (2026-10-13)

`config validate --config <absolute-path>` accepts exactly one config option before,
between or after the two command words. Unknown/duplicate flags, extra values,
relative or non-Unicode paths fail before file access with the invocation diagnostic.
`config validate --help` (help before/between words also accepted) loads no resources.
Global help now lists this command. Webhook validation succeeds with schema/cluster/
count output; invalid config/resources or unsupported SMTP scope exit 2 with the
corresponding static stderr snapshot and empty stdout. No monitoring command is active.

## SMTP-validation addition (2026-10-13)

Configuration validation now supports webhook, SMTP and mixed configs. Success
with SMTP reports `smtp_permissions=approved`. A denied/mismatched current pair
returns `config-permission.stderr`, empty stdout, exit 3; config/resource failure
still uses `config-invalid.stderr`, exit 2. The earlier unavailable-SMTP diagnostic
is retired. No network/SMTP submission or state inspection occurs.
