# 0059: Continuous scheduling within one owned foreground session

Date: 2026-10-13
Status: accepted internal implementation

## Decision

Run the continuous scheduler inside the existing foreground session. The prepared
resources, database connection and exclusive ownership guard remain alive across
all scheduling and poll batches. Explicit renewal accepts 1..1024 polls and refills
only an exhausted batch. It changes neither transient failure/checkpoint health nor
persistent reconciliation, pending deliveries or deduplication. Finite runs keep
their original budgets and behavior.

Scheduling cancellation, tick failure, wait failure and invalid renewal policy
close the session and release ownership. Cancellation is checked by the shared
scheduler before renewal/tick; health renewal cannot clear sticky cancellation.
Blocked delivery still observes completion-relative delay and does not consume
an invented send attempt. This is an internal API; installed monitoring remains
unavailable and webhook-only session composition is still required.

## Acceptance

Unit tests verify renewal retains failure/checkpoint health, never replenishes an
unfinished batch, and rejects invalid limits without mutation. Closed sessions and
invalid batch sizes execute no hooks. Process acceptance runs 65 blocked-delivery
ticks across two-poll batches using injected time, verifies the poll keeps running
and ownership stays exclusive across renewals, then cancels and reopens. A wait
failure after renewal also releases ownership and preserves pending work for replay.
Existing finite, real-wait, SIGINT/SIGTERM and recovery scenarios remain in the
same process harness. No cluster or provider requests are made by these fixtures.

## Remaining MVP work

Bind retained signal reasons to installed exits; connect OS-backed continuous
orchestration; add webhook-only sessions, public frozen event transactions,
installed config/status commands and setup/test/doctor. Local MVP contracts are
adopted; product/platform/source-data and external acceptance remain separate work.
