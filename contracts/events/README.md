# Webhook event payload 1.0

The local MVP event contract was adopted on 2026-10-13. This increment provides
an internal, transactional webhook producer/persistence path and sender schema;
it introduces no installed monitoring or delivery command.

`webhook-v1.schema.json` describes the current sender shape and lifecycle/severity
mapping. Receivers should tolerate additive unknown fields within major version 1,
and reject/handle unsupported event types deliberately. Incompatible type, meaning,
required-field or privacy changes need a new major version. No automatic snapshot
regeneration. Package/config/database/payload versions are independent.

Public IDs are `evt_` plus UUID-v4 lowercase hex. Generation uses the existing
ring SystemRandom source for 16 random bytes, then sets UUID version/variant bits.
IDs are independent of private attempt identifiers. Eight collision attempts are
allowed inside the SQLite write transaction; entropy failure or collision exhaustion
fails and rolls back. No new dependency or guessed numeric Slurm ID width is added.

Payloads include only schema/public ID, source, safe configured cluster alias,
event/severity, first committed observation time and minimal job fields. Allocation
and optional array-task IDs are canonical decimal strings; allocation must be
positive, task zero is valid. Steps/ranges/heterogeneous/noncanonical identifiers
are rejected. The encoded frame cap is 16,384 bytes, not a numeric-width guarantee.
Prepared provider limits are also checked before admitting a public poll.

The current producer always emits null source occurrence/submission/start times:
private attempt-context strings do not establish independent source timestamps or
timezones. It never substitutes observation time for source time. It emits no user,
job name, command, path, URL, credential, exit code or arbitrary scheduler details.

## Atomic freezing and state compatibility

The explicit public-webhook path stores the internal deduplication ledger, jobs,
selected outbox rows, public UUID, JSON bytes, SHA-256 integrity hash, selected route
revision bindings and checkpoint in one transaction. Schema 5 is created by that
same transaction only when a new routed public event is first committed. Version-4
private paths do not upgrade automatically. A failed poll rolls back both migration
and data. Existing private rows are never backfilled or silently made public.

Immutable SQLite triggers seal the route set when its frame is inserted and forbid
frame/route updates or deletion. Retiring an outbox row does not rewrite the retained
frame/bindings or deduplication history. Replays cannot add destinations or change
bytes. Delivery lookup checks the requested revision, UUID, bounded bytes, integrity
hash and payload/public-ID agreement; old private rows return no public frame.
Installed historical status reads versions 4 and 5 without migrating either.

A webhook revision binds exact resolved endpoint bytes and URL/auth environment
reference names using a versioned SHA-256 domain. It stores no plaintext endpoint
or authorization. Credential value rotation under the same authorization reference
does not change endpoint identity; endpoint or reference changes do. Resources are
prepared before the poll, remain owned, and public composition accepts HTTPS only.
Changed/missing resources must block old pending work rather than retarget it.

## Remaining activation work

Mixed-provider storage preparation can commit public webhook frames alongside
frozen SMTP messages and rendering/provider bindings in one poll transaction. Only
webhook destinations receive public frame route rows; SMTP retains its existing
message and profile bindings. Mixed poll composition now verifies complete prepared HTTPS bindings before querying
and admits selected payloads against provider limits before commit. Caller state
advances only after commit. Supervised runtime cycles share the owned foreground session.
The stored-frame adapter checks HTTPS, current endpoint revision and payload limits
again before constructing an owned secret-stdin request. Controlled children prove
exact public UUID/bytes, completion-based retry and restart; changed resources block
dispatch without consuming attempts. Mixed delivery composition uses the checked retry worker for both this adapter
and permission-guarded SMTP, with destination-specific transport deadlines. Partial
completion stays committed when another destination is blocked; denied destinations
consume no attempt and are resumable after configuration/permission repair. Public poll/delivery now share runtime-cycle preflight, bounded poll budgets,
checkpoint planning, health, cancellation and maintenance. Cancellation after query
completion preserves the atomic poll while stopping before delivery. Owned continuous sessions now retain the prepared plan and single state owner
across public poll-budget renewal. Controlled child acceptance verifies retry and
cancellation recovery after configuration removal. Installed SMTP/mixed runs have controlled SMTP submission and store-restart
acceptance; installed verified HTTPS retry/submission/restart also has controlled acceptance. At-least-once replay can still duplicate a provider-accepted request
whose acknowledgment/local completion was lost; no exactly-once/human-receipt claim.
Interactive setup/permission review is installed. Authorized cluster/platform/source-data
acceptance remains a support gate.

Explicit `test` uses [test-v1.schema.json](test-v1.schema.json), a separate labelled
synthetic payload with no cluster/job data. It is not a durable monitoring event.

## Slack rendering v1

`slack-v1.schema.json` defines the separate Slack incoming webhook body. It does
not change the generic lifecycle/test schemas. Lifecycle text is exactly:

```text
Slurm Notify: <state>
Cluster: <cluster_id>
Job: <allocation_id>[_<array_task_id>]
Observed: <observed_at>
Event: <event_id>
```

Synthetic text is exactly `Slurm Notify test`, then
`Synthetic Slurm Notify test; no scheduler data.`, then `Event: <event_id>`,
separated by newlines. No trailing newline. Formatting/unfurl properties are false.
Only existing allowlisted fields are used. Text is capped at 4000 bytes; canonical
and rendered bytes must each fit the destination payload cap. Rendering is stable
for a frozen frame and bound to a versioned destination revision; changing that
revision blocks pending work. Slack may duplicate messages after lost acknowledgements.
