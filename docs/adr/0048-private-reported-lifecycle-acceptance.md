# ADR 0048: Combined private reported lifecycle acceptance

Status: trusted Linux/macOS acceptance; no installed lifecycle entry point.

## Evidence

Combined child modes use protected reported startup, shared-signal active ticks,
real monotonic scheduling and bounded shutdown classification. Parent acceptance
pauses a query/send callback and atomically replaces configuration with malformed
protected input. The already-prepared session continues from its owned plan and
reports cancellation while preserving committed frozen work or partial delivery.

A fresh child reads the replacement and reports preparation failure; state filenames
and bytes remain unchanged. Restoring valid configuration allows a new child to
report finite exhaustion after a deduplicated tick. Frozen SMTP bytes, event ID,
provider binding, attempt and checkpoint survive query recovery. Ordinary delivery
recovery sends only the remaining item and leaves no pending work. Read-only status
is fresh after shutdown. Exact bounded supervision rejects other output.

## Limits and next

The modes are test-only. Callbacks simulate query/provider success and observe
cancellation at existing stage/batch boundaries. Source replacement does not reload
an active plan, and exhaustion does not imply all blocked deliveries completed.
No supported CLI, public configuration, dependencies or product defaults change.

Next: private startup-to-shutdown orchestration adapter returning bounded startup
or shutdown reports, with explicit caller-owned hooks and recovery acceptance.
Public contracts, licensing and installed activation remain review gates.
