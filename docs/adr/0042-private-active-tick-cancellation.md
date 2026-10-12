# ADR 0042: Private shared cancellation through active ticks

Status: private composed adapter; no installed command or signal handling.

## Decision and evidence

An interruptible tick adapter accepts the caller-owned Signal in its hook bundle
and delegates to existing composed cycle cancellation checks. It uses the same
fail-closed poisoned-state policy as scheduling. Callers can use one signal for
scheduled waits and active poll/delivery stage boundaries. Cancelled reports close
the session and release ownership; closed sessions retain existing rejection rules.

Two tests cover cancellation during injected callbacks. Cancellation in the
accounting query allows the already-running poll to finish and commit one frozen
SMTP event, then skips delivery and cleanup. Restart with the sticky signal runs no
query/clock/provider hook and preserves frozen bytes and the pending attempt.

Cancellation in an ordinary send callback records its known successful delivery,
stops before the next pending send and skips cleanup. The report carries a cancelled
batch error with committed progress. Restart with a fresh signal reconciles without
duplicate events and delivers exactly the remaining item. Ownership can be
reacquired after both cancelled ticks.

## Limits and next

Cancellation is cooperative at existing stage/batch boundaries. It neither kills
an in-flight query/transport nor rolls back a completed poll or successful send.
Callbacks retain their own execution deadlines and uncertain-send handling. A
shared signal must be passed explicitly to both scheduling and tick adapters.
No new dependencies, public contracts, product defaults or provider access result.

Next: process-boundary shared cancellation during active composed ticks, preserving
partial delivery progress and pending frozen work across restart.
Public contracts, licensing and installed activation remain review gates.
