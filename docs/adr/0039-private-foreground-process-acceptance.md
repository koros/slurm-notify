# ADR 0039: Private foreground process acceptance

Status: trusted Linux/macOS child fixtures; no installed flags or signal adapter.

## Decision

The custom process harness composes the protected mixed plan, owned session,
monotonic schedule and selected cycle in a trusted child. Synthetic scheduler output
commits one terminal event with frozen SMTP rendering/provider binding. An
unenrolled permission blocks dispatch before credential or helper access. The child
acknowledges its first scheduled wait while retaining guarded state ownership.

The parent verifies another process cannot inspect owned state, then triggers
cooperative cancellation, forced termination or an injected wait error. Existing
bounded READY/output handling, cleared environments and reap-on-failure cleanup
are shared with status acceptance. Child tokens remain exact static fixture output
within 64 bytes, with stderr suppressed. One-shot replay uses the existing supervised
transport and two-second deadline.

## Evidence and limits

Pre-start cancellation with missing configuration creates no state. After each
held-wait exit, state filenames and bytes match the snapshot taken while idle.
Reacquisition succeeds. A new child schedules two more blocked ticks and exhausts
its finite budget. Event identity, pending attempt number, frozen SMTP bytes,
provider binding and durable checkpoint match before and after replay, with only
one pending delivery. Read-only inspection then reports fresh durable state.
Missing configuration produces a static invalid token without private diagnostics.

The pending work comes from the composed child tick rather than a parent-seeded
ledger. Forced termination happens during an idle wait after commit; this is not
proof of termination during a SQLite write or external send. Trusted held fixtures
spawn no descendants and are cleaned up by the parent; supervised process-group
cleanup has separate existing acceptance. Waiting is controlled through stdin and
synthetic time. No production signal handling, real scheduler, network delivery or
installed foreground agent is claimed. No dependencies or public contracts change.

## Next

Add a private bounded interruptible wait adapter using a caller-owned cancellation
signal, then integrate it with scheduled sessions using deterministic acceptance.
Public contracts, licensing and installed activation remain review gates.
