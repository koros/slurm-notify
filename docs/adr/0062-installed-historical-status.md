# 0062: Installed historical status inspection

Date: 2026-10-23
Status: accepted local MVP increment

## Decision and compatibility

Add `status --config <absolute-path>` and resource-free `status --help`. Exactly
one config option is required before or after the command. Existing version,
configuration-validation and invocation-error streams remain unchanged; global
help expands deliberately in the unreleased 0.0.0 package. Record exact empty-state
and failure snapshots under contracts/cli.

Parse protected configuration metadata only. Do not resolve provider URLs, trust
files, usernames, passwords or account names. Inspect existing schema-4 state at
the current fixed `state.sqlite3` basename, using an existing owner lock, an owned
mode-0700 directory, mode-0600 regular files, no symlinks and no recovery sidecars.
Refuse active/cooperating owners; do not create directories, locks, databases,
migrations or repairs. This is offline historical inspection, not a process-health
endpoint. Missing or incomplete state, busy state, unsafe/recovery state, unsupported
schema and query/checkpoint errors have distinct static diagnostics. Config errors
exit 2, state/inspection failures exit 1, successful historical inspection exits 0.

Extract the existing read-only connection setup and bounded health transaction into
shared modules. Private status/store APIs delegate to those same routines. Keep the
SQLite connection before its ownership guard in struct field order so SQLite closes
before ownership is released, including error/panic unwinding. Preserve the existing
checkpoint parser through a shared UTC timestamp module. Add a retrying sample count
for pending destinations whose latest attempt is a transient failure.

Output labels process health unknown and all queue/capacity/retention counts sampled.
Truncation and capacity pressure are explicit; totals, running-process health and
transient in-memory failure counters are not invented. Missing checkpoint means
unknown age/stale; invalid/future checkpoint fails closed. Existing numeric query
limits remain explicit configuration values.

Only the already locked rusqlite/time versions move to normal dependencies. Enable
rusqlite's hooks feature for cooperative VM interruption using the configured query
timeout; SQLite busy waiting uses that timeout separately. This is not a universal
filesystem/startup hard deadline. No new package/version or state migration is added.

## Acceptance

Installed-binary tests cover absent state without creation, empty closed state,
byte-for-byte unchanged databases, active-owner refusal then successful reopening,
queued/retrying/failed samples, unproven completion, fresh/stale/future checkpoints,
truncation, unsupported schema, unsafe mode, symlinks and untouched WAL sidecars.
A mixed configuration with unavailable provider resources still permits inspection.
A deterministic SQL read-work test verifies zero-budget interruption and safe handler
removal. Existing private storage/status/accounting fixtures exercise shared behavior.

## Remaining MVP work

Live status during monitoring remains unsupported; busy output instructs users to
stop the owner before historical inspection. Public event persistence, installed
monitoring/delivery and setup/test/doctor still require implementation and acceptance.
