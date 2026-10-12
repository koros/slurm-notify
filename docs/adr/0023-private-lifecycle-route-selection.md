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

This selects destinations only. It does not alter durable event preparation,
pending-route bindings, permissions, disclosure or sending. A later persistence
adapter must select destinations before atomically enqueueing an event; applying
current rules to already pending work is not authorized by this experiment.

## Next

Prepare private atomic event-specific enqueueing with route-selection acceptance
and restart preservation. Installed mapping and activation require explicit review.
