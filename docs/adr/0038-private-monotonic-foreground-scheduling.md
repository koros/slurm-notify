# ADR 0038: Private monotonic foreground scheduling

Status: private finite scheduling adapter; no installed runtime or signal handling.

## Decision

Owned sessions can run a caller-supplied tick closure using the prepared plan's
poll interval, injected monotonic clock, interruptible wait and cancellation probe.
The first tick starts immediately. Later ticks wait one interval after the preceding
tick completes. Slow ticks and oversleep never trigger catch-up bursts. Early wakes
recompute the remaining delay. Interval bounds are 1 second through 24 hours; each
run permits 1 through 1024 ticks and at most 1024 wakes between ticks.

Missing/regressed clocks, deadline overflow, failed waits/ticks, zero-progress wakes
and wake-budget exhaustion return static errors. Cancellation before ticks, after
ticks and around waiting returns a count of completed tick calls. A cancelled tick
counts as one call. Exhaustion returns after the final tick without a final wait.
Every session scheduling exit, including policy/error exits, closes ownership.
Closed sessions reject scheduling before calling hooks.

Blocked delivery stage reports do not end scheduling: existing cycles continue to
apply their permission, query, delivery and maintenance rules. The tick closure
observes each report; the scheduler retains no reports or private event metadata.
The session's existing poll budget and the scheduler's tick budget remain distinct.

## Evidence and limits

Synthetic time tests verify long ticks, early wakeups, oversleep, finite calls and
no trailing wait. Failure tests cover cancellation, invalid policies, missing and
regressed clocks, overflow, wait/tick errors, no progress and tiny-progress wake
exhaustion. Session tests verify cancellation while waiting and policy/clock/wait
errors release the lock, with closed hooks rejected. Restart acceptance schedules
two permission-blocked batches, preserving the pending frozen message, attempt
number and deduplicated event, then reacquires owned state after exhaustion.

Injected hooks must obey their own bounds; the scheduler cannot preempt synchronous
work. Tests use synthetic monotonic time and no cluster/network access. There is no
OS signal adapter, built-in sleep loop, hot reload, background service or certified
wall-clock runtime. Public contracts and executable defaults remain unchanged.

## Next

Add private process-boundary foreground scheduling acceptance for cancellation
while waiting and forced owner termination, with durable pending-work recovery.
Public contracts, licensing and installed activation remain review gates.
