# ADR 0021: Private operational-control mapping

Status: private experiment; no approved configuration schema or installed path.

## Decision

Add test-only `runtime_mapping` to accept explicit typed numeric controls and
construct the existing accounting-window policy, retry policy and maintenance
settings, plus polling/query durations, output/identity/event capacities and a
revision-cleanup limit. Add no dependencies, lookup side effects or defaults.

Use the current draft ranges for 17 numeric controls. Check all ranges and the
five polling/window/stale/backoff/jitter relationships before returning a plan.
Convert sizes with checked usize conversion. Signed/u32 casts follow validated
ceilings below their representable maxima. Parse the explicit jitter seed from
1..20 ASCII decimal digits into u64; reject overflow, signs, whitespace, fractions,
exponents and non-ASCII text. Leading zeros remain accepted by the draft grammar.

The caller supplies every field. Construction publishes a complete typed plan only
on success. Static limit/relationship/seed categories do not echo input. The ranges
are private preparation based on review drafts, not approval of those drafts or
cluster policy recommendations.

## Evidence and limits

Three tests verify exact mapping of representative explicit values, valid combined
minimum/maximum settings, maximum u64 seed preservation, runtime retry/cycle
compatibility, below/above-ceiling and u64::MAX failures across every numeric control,
all five invalid relationships and invalid decimal seed forms.

An additional runtime-cycle acceptance test uses mapped query/output/window,
capacity, retry, health and retention settings with a temporary SQLite store.
Query callbacks receive a remaining deadline no greater than the mapped timeout.
A retry becomes due at the configured backoff, survives reopen with the same event
and attempt, and recovers while a poll fails. The persisted checkpoint makes health
stale at the configured threshold; completed work stays retained until its interval
elapses, then cleanup removes it without replay. Remapping the same explicit controls
after reopen preserves behavior. Send/query callbacks are synthetic; there is no
Slurm, external network access or installed schedule.

There is no JSON parsing, file/environment access, polling loop, scheduler command,
state creation, provider selection or resource resolution. The mapping does not
establish supported paths, source/user identity, initial observation, event filters,
route preparation, SMTP permissions or public event semantics. Those remain separate
preparation/review tasks. Poll duration is a value, not an activated schedule.

## Next

Prepare the remaining private identity/observation and routing mapping boundaries.
Installed parser/configuration mapping and activation still require explicit review.
