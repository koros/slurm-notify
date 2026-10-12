# ADR 0033: Private composed status snapshots

Status: private health adapter; no installed status command or output contract.

## Decision

A prepared mixed plan can request status from caller-owned state and a caller-driven
cycle without providing query, identity, credential, transport or clock callbacks.
It maps cluster, independent capacities, stale threshold, retention and health sample
bounds from its already validated metadata. The caller supplies the observation time.

Storage health and the durable checkpoint are read in one SQLite read transaction.
The checkpoint projection forwards only the bounded 19-character timestamp shape;
other lengths fail statically. Runtime parsing validates timestamp semantics. Health
reports bounded samples and pressure flags, never job identities or provider resources.
Status computes freshness from the durable checkpoint after restart while retaining
only the current cycle's in-memory failure counters. Those counters are not persisted.

Unlike cycle restoration, status remains available when the configured capacity is
smaller than the retained ledger. It reports pressure without truncating or deleting
history. No checkpoint is treated as stale with unknown age. The stale boundary is
inclusive; negative time, invalid policy, clock regression, corrupt timestamp and
storage errors remain explicit static categories.

Existing health adapters reuse the sampling implementation without reading checkpoint
metadata, preserving their error ordering. Cycle reports still restore and validate
their ledger before reporting.
This private change introduces no public format, flags or CLI behavior.

## Evidence and limits

Two composed tests cover empty state, two observed lifecycle events, owned-state
reopen, fresh/stale boundaries, event-only pressure under a reduced budget, backwards
and negative clocks, corrupt/missing checkpoint schema and invalid internal policy.
Repeated status reads preserve the pending event, attempt number and frozen SMTP
bytes. Debug inspection confirms the report excludes fixture identity, cluster,
provider, resource-reference and path values. No status callbacks can resolve or send.
Existing health tests continue to verify delivery sampling and truncation behavior.

An already prepared mixed plan still eagerly resolved webhook resources and SMTP
trust during preparation. This milestone does not make configuration preparation
resource-free. Reporting adds no ambient environment access, relay receipt, real
cluster query, automatic pruning, foreground loop or installed monitoring.

## Next

Add private metadata-only status preparation from protected configuration snapshots
so resource unavailability does not prevent health inspection. Public contract,
licensing and installed activation gates remain explicit.
