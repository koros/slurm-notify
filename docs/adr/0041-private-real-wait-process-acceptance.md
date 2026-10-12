# ADR 0041: Private real-wait process acceptance

Status: trusted Linux/macOS fixtures; no installed foreground command or signals.

## Decision and evidence

Extend foreground process acceptance with the owned session's real Instant and
condition-variable adapter. After committing one permission-blocked SMTP event,
the child acknowledges its first tick and a control thread converts parent stdin
into shared sticky cancellation. Cancellation can arrive around wait registration;
the same mutex protocol handles either ordering. Forced process death during this
idle period is also tested. The existing parent deadline/output/reaping boundaries
apply, and diagnostics remain exact static fixture tokens.

After cooperative cancellation and forced termination, the parent verifies owner
release and byte-for-byte idle state preservation. Child replay still preserves one
pending event, its attempt number, frozen bytes, provider binding and checkpoint.
A separate validated fixture changes only polling/query controls to two/one seconds;
real-time replay runs two blocked ticks, waits at least two seconds and exhausts its
budget without altering durable pending work. One-shot supervision allows five
seconds for this bounded fixture, rather than the previous two-second deadline.

## Limits and next

The control thread is test-only, reads one byte or EOF and spawns no descendants.
Cancellation/termination occurs after commit, not during writes or sending. No OS
signal handler, cluster access, provider delivery or installed runtime is introduced.
Requested wait durations do not certify OS scheduling latency. No dependencies or
product defaults change.

Next: private foreground cancellation propagation through composed query/delivery
hooks, including cancellation during an active tick and pending-work recovery.
Public contracts, licensing and installed activation remain review gates.
