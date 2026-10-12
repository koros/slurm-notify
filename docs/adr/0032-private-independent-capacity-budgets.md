# ADR 0032: Private independent identity and event budgets

Status: private runtime/storage integration; no public contract changes.

## Decision

Carry configured identity and event capacities independently through composed cycle
settings, reconciliation, bounded SQLite restoration, capacity preflight and health.
Each table probe reads at most its own limit plus one row. Each restored ledger
rejects overflow instead of truncating history. Candidate reconciliation checks both
limits before publishing any change, so new work that exceeds either budget cannot
partially advance the checkpoint, event ledger or outbox.

Existing shared-capacity reconciliation, restore and health adapters delegate with
the same limit for both categories. Existing cycle fixtures explicitly retain their
shared values; composed configuration and mapped-control fixtures use the separately
validated event limit. Runtime preflight validates both positive, safely probeable
capacities before clocks, storage, query or send hooks. No migration or database
schema change is required.

A smaller configured budget may block an existing ledger at preflight. Raising the
relevant limit and reopening restores all retained work without retargeting or
resetting attempts. This does not automatically prune data to meet a lower budget.
Poll errors retain existing behavior: already durable delivery and maintenance can
continue when the cycle's existing stage policy permits it.

## Evidence and limits

A protected-file/owned-state composed test observes a running job, then its terminal
event at the event boundary while retaining pending ordinary delivery. Lowering the
event budget after reopen blocks with event-only pressure before querying or sending.
Restoring the exact budget delivers the unchanged pending item once. A new job's
running event beyond the event limit fails atomically; increasing that limit allows
recovery. A further identity beyond its own limit also fails atomically. Checkpoints,
retained state and event counts are checked through restoration.

Storage tests verify shared-adapter equivalence and independent zero/overflow probe
validation. Runtime invalid-policy acceptance now covers zero and oversized event
budgets before hooks. Existing shared-budget pressure/recovery tests remain intact.

Health probes bound returned rows rather than SQLite scan time. This change adds no
automatic retention, foreground scheduling, public status format or installed agent.
No real cluster or external receipt is claimed.

## Next

Compose private health/status snapshots from validated plans and owned state,
including stale checkpoints, capacity pressure and credential-free reporting.
Public contract, licensing and installed activation gates remain explicit.
