# ADR 0037: Private owned foreground session

Status: private Linux/macOS adapter; caller-driven ticks, no installed command.

## Decision

A session owns one protected-file mixed configuration plan, guarded writable Store
and caller-bounded Cycles object. Startup checks cancellation before preparation,
after preparation and after state acquisition. A zero poll budget fails before
resource access. Preparation/acquisition errors become static classifications.
No identity, credential, trust or query adapter is implicitly selected.

Session ticks delegate to the existing composed cycle implementation and status
reads use its coherent health adapter and accumulated ephemeral poll failures.
A cancelled tick closes the session and drops both resources and the owner guard.
Explicit stop is idempotent; dropping a session also releases ownership. Closed
sessions reject status and ticks before invoking hooks. Other cycle/status errors
leave the session active for caller-directed recovery or shutdown.

## Evidence and limits

Two tests cover invalid limits/missing files without lookups, cancellation before
preparation/acquisition and after acquisition, lock release, repeated stop and
closed operations. Startup cancellation after writable acquisition can leave the
initialized private state namespace; it does not roll back initialization.

A second owner is rejected while the original session remains usable. Removing
configuration after startup does not invalidate the owned plan. A selected event
creates frozen pending SMTP work; an unenrolled pair blocks secret resolution and
sending. Query failure appears in live status. Cancellation releases ownership,
restart resets ephemeral failure counters, durable checkpoint freshness survives,
and replay produces no duplicate event or changed frozen message/attempt number.

The poll budget keeps existing Cycles semantics: exhausted polling may still allow
delivery and maintenance. It is not a bound on the number of session method calls.
Cancellation remains cooperative between stages; synchronous preparation, SQLite
and supplied hooks are not preempted by the session. The session has no timers,
signal handlers, reload, daemon/service installation or certified wall-clock bound.
No dependencies, executable defaults or public contracts change.

## Next

Add private monotonic foreground scheduling with explicit finite tick budgets and
interruptible caller-supplied waiting, including delayed ticks and blocked batches.
Public contracts, licensing and installed activation remain review gates.
