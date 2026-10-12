# ADR 0040: Private interruptible foreground wait

Status: private caller-owned cancellation; no installed signal or runtime contract.

## Decision

A caller-owned Signal shares a sticky cancellation bit, mutex and condition variable
across cloned handles. Cancellation sets the bit under the same mutex used to check
and register waits, then wakes every waiter. Repeated cancellation is harmless;
there is no reset. Starting a new independent run requires a new signal.

Waits accept positive durations through 24 hours, return immediately when cancelled
and otherwise use one condition-variable timeout. A spurious wake returns Ready;
the existing monotonic scheduler recalculates the remaining delay and enforces its
wake budget. Poisoned state and invalid durations produce static errors.

The owned session's interruptible adapter uses Instant elapsed time, this wait and
the same signal for cancellation probes. Cancellation-state errors fail closed,
preventing another tick; a wait error maps to the scheduler's static wait failure.
Every scheduling exit retains the existing session shutdown behavior. Callers must
also use the shared signal inside tick hooks if they need cancellation during a tick.

## Evidence and limits

Four wait tests cover duration validation, shared sticky cancellation, uncancelled
timeout, cancellation of multiple concurrent waiters and poisoned state. An owned
session test cancels a real-time scheduled run from another thread after its first
tick, checks closed status and lock reacquisition, then restarts with the same sticky
signal and verifies no tick executes. Closed sessions reject the adapter's hooks.
Cancellation around wait registration is race-safe; tests allow cancellation to arrive
before registration as well as during waiting.

Requested durations bound each condition-variable wait, not OS scheduling latency
or mutex acquisition in a compromised process. The adapter does not preempt SQLite,
preparation or tick hooks, and provides no async-signal-safe handler. There is no OS
signal registration, service installation, supported daemon or real-cluster claim.
No dependencies, public contracts or executable defaults change.

## Next

Use the real monotonic/wait adapter in trusted process-boundary foreground fixtures,
including cancellation, forced termination and durable pending-work replay.
Public contracts, licensing and installed activation remain review gates.
