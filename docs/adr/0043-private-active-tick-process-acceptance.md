# ADR 0043: Private active-tick process cancellation

Status: trusted Linux/macOS fixtures; no installed signal or provider protocol.

## Decision and evidence

Trusted foreground children pause inside an injected accounting query or ordinary
send callback. The parent verifies guarded ownership through a separate process,
then supplies one byte to request shared sticky cancellation. EOF also cancels.
Existing bounded child coordination, static stdout, cleared environments and
reap-on-failure cleanup apply. No actual scheduler or provider is contacted.

The query callback completes after cancellation and the poll commits one frozen
SMTP event, then skips delivery and cleanup. The parent reacquires state and checks
one pending attempt. A fresh child reconciles with zero duplicate events; its
permission-blocked tick leaves the event ID, frozen bytes, provider binding,
attempt number and checkpoint unchanged.

The send callback returns a known successful result after cancellation. The batch
records one committed delivery, stops before the next item and skips cleanup.
Parent inspection finds exactly one pending first attempt. A new child polls with
zero new events and delivers only that remaining item; no pending work remains.
Both paths close ownership and allow fresh read-only status after replay.

## Limits and next

The stdin coordination and successful send are fixtures, not OS signals or evidence
of external provider acceptance. Cancellation occurs within bounded parent-controlled
callbacks and is observed at existing stage/batch boundaries; it does not preempt
query/transport execution. Forced termination during sends and ambiguous acceptance
retain separate existing supervisor/durable-send evidence. Public contracts,
product defaults and dependencies remain unchanged.

Next: private foreground shutdown outcome classification and bounded redacted
reporting for cancellation, exhaustion and failures, without installed activation.
Public contracts, licensing and installed activation remain review gates.
