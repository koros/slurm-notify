# ADR 0023: Private lifecycle route selection

Status: private experiment; no approved route or event contract.

## Decision

Compile caller-selected providers and rules into nine destination masks. Require
1..64 unique bounded aliases, at most 128 rules, 1..9 known event names per rule
and 1..64 existing destinations per rule. Reject repeated events/destinations
within a rule; overlapping rules union each destination once. Return destinations
in provider-list order, independent of rule order. Empty rules explicitly select
nothing. Pending/unknown scheduler states select nothing.

Keep aliases in owned zeroizing strings without Debug/Clone. Static limit,
reference and duplicate errors do not echo private aliases. The nine name/state
mappings follow current review drafts and private lifecycle states, without
freezing public meanings or event payloads. No dependency or default changes.

## Evidence and limits

Three tests cover source removal, overlapping unions and deterministic order, all
nine lifecycle kinds, no-match/empty rules, all 64 destination bits, 128-rule
acceptance, exceeded ceilings, duplicates and unknown references. No filesystem,
scheduler, resource lookup or network access is required.

The private polling adapter now selects each newly deliverable event's destinations
before the existing atomic storage transaction. Candidate reconciliation publishes
only after commit. The existing uniform-destination and SMTP paths delegate through
the same storage builder with their original destination selections.

An additional temporary-SQLite acceptance test routes completed and failed events
separately while retaining an unmatched timeout in the deduplication ledger. A
synthetic outbox trigger rejects one destination; jobs, event ledger, outbox and
checkpoint changes roll back and the in-memory candidate remains unpublished.
Removing the trigger allows recovery. Reopening with replacement rules preserves
the original pending destinations and attempts; it neither retargets those records
nor backfills the previously unmatched event.

The private bound SMTP plan also accepts per-event destination selection. It
validates subsets, duplicates and count ceilings before rendering, counts only
selected SMTP pairs against the preparation budget, then commits ordinary pairs,
frozen SMTP bytes and rendering/provider bindings through the same transaction.
Storage independently checks selected membership and rejects unselected messages.
The existing uniform preparation paths retain their behavior.

Mixed acceptance selects one SMTP completed event and one ordinary failed event
while leaving a timeout unmatched. A one-message ceiling succeeds although the
unfiltered Cartesian product would exceed it. Invalid selections and oversized
preparation fail before publishing candidate state. A synthetic smtp_messages
trigger verifies write rollback; removing it recovers. Reopening with changed
selection preserves the pending routes, frozen bytes and provider binding.

This prepares private ordinary/SMTP outbox routes and existing SMTP frozen bytes
and profiles, without public webhook payload freezing, permission grants,
disclosure changes or sending.
The storage selector callback is private and caller-managed; the route plan bounds
its own output, without certifying arbitrary callbacks. Current rules must not be
applied to already pending work to retarget it.

The mixed acceptance fixture now also exercises guarded dispatch with synthetic
provider callbacks. Ordinary delivery completes while SMTP retries. After reopen,
permission denial leaves the pending SMTP attempt unchanged; a later permitted
attempt uses rotated credentials with the original frozen message and bindings.
The completed ordinary event is not replayed and the unmatched timeout stays in
the ledger. These callbacks do not perform network access.

## Next

Exercise selected mixed routes through bounded runtime-cycle dispatch/restart
acceptance. Installed mapping and activation require explicit contract review.
