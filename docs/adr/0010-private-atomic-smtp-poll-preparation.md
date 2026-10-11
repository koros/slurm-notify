# ADR 0010: atomic private SMTP poll preparation

Status: private storage experiment; no supported persistence or provider contract.

## Decision

Add a separate prepared poll path accepting a borrowed candidate reconciler,
deliverable event slice, destination names, explicit SMTP destination subset and
validated Prepared messages keyed by lifecycle event/destination. Message construction
happens before storage begins; this method accepts no renderer or network callback.
Batch/preparation types have no Debug/Clone and borrow message buffers instead of
copying message DATA. Credential and relay configuration stay outside persistence.

Before writes, reject empty/duplicate destinations, invalid/duplicate SMTP subset
entries, duplicate deliverable events, deliverables outside the candidate ledger,
and duplicate/unrouted/unrequested preparations. Within BEGIN IMMEDIATE, each new
eligible event/destination inserts its outbox row and consumes exactly one supplied
SMTP message. A missing preparation fails; any unused preparation also fails,
including a preparation for a previously committed event. Insert the frozen
validated envelope/DATA in that same transaction. Commit jobs, event ledger,
outbox rows, frozen messages and checkpoint only after all pairs succeed.

Existing promotion, event identity conflict checks and checkpoint monotonicity
remain shared. Any error drops the transaction: no new deliveries, partial
messages, job updates/promotions or checkpoint advancement become visible. No
nested freeze_smtp transaction or connection is used. The SQLite commit precedes
any later delivery. Per-message address/DATA bounds remain those of ADR 0008;
caller reconciliation and routing bounds govern batch size. This does not add a
product template, event rendering policy or public route discriminator.

## Replay and legacy behavior

Historical ledger events keep their existing outbox routes and frozen bytes.
Replay supplies no new preparation for them, even if the current routes differ.
Suppressed discovered events remain ledger-only. The prepared path does not
backfill missing legacy snapshots, reset attempts or recreate cleaned deliveries.
A surplus newly rendered historical candidate causes rollback rather than replacing
a stored message. Existing missing/corrupt snapshot dispatch blocking still applies.

The earlier private persist_reconciler path remains for non-SMTP scheduler and
legacy fixtures, with its original SQL failure behavior. Only explicitly selected
SMTP destinations in the new prepared path receive the atomic snapshot invariant;
this increment does not activate production polling or provider selection. The
standalone freeze_smtp experiment remains useful for legacy fixture preparation.
Schema stays at private version 2; no dependency/default changes are introduced.

## Verification and next step

Tests commit two events to a plain route and two SMTP routes with distinct recipient
messages; reopen checks exact frozen bytes, ledger/checkpoint and completion-based
retry history. Historical route changes and conflicting fresh preparations cannot
replace bytes or replay deliveries. Missing, extra, duplicate and invalid batches
leave empty state, including a missing message on another SMTP route. Faults on the
second message insertion and final checkpoint write preserve the previous pending
job/checkpoint through reopen, then allow a clean retry. Legacy events cannot be
backfilled through this path, and regressed checkpoints fail.

The owned-store TLS submission fixture now commits its event/outbox/message together
before sending. Temporary refusal and relay-accepted/lost-child-result replay still
use the same stored envelope/DATA after reopen. No real relay or Slurm service is used.

Next: compose prepared storage with private durable scheduler polling using a
fallible bounded fixture preparation callback; install the in-memory candidate only
after commit. Event rendering/redaction, route revisions, credentials, public
review, licensing, real-cluster validation and runtime activation remain pending.


## Subsequent scheduler composition

The private smtp_poll module now composes the shared bounded query/parser with
atomic prepared storage. It validates routes and an explicit maximum message count,
clones the reconciler, queries/reconciles, checks new-event/SMTP-route cardinality,
prepares bounded messages locally, persists the batch, and installs the candidate
only after successful commit. Historical/suppressed events never call preparation.
Callbacks return only a static preparation failure, and batch values do not expose
message Debug/Clone output. Prepared DATA retains its existing 16,384-byte cap;
count limits govern retained message payloads, not all upstream heap allocations.

Preparation is synchronous fixture work. The API bounds callback count and message
size, and retains the existing scheduler query deadline; it cannot interrupt an
arbitrary blocking renderer or promise a deadline for the entire poll/preparation.
No network or preparation callback is invoked inside the storage transaction.
The caller must restore durable state before beginning or retrying after reopen.
The existing plain poll and its error categories remain unchanged.

Four tests cover mixed routes and stored recipients/DATA, restored replay with
changed routes and zero preparation calls, limits before callbacks, later preparation
failure without writes, a late checkpoint failure preserving a previous pending
job/checkpoint in both memory and SQLite, retry success, settings/query/parser errors
and suppressed discovery. The owned TLS fixture now runs the composed private
query/reconciliation/preparation/commit path before real child submission and frozen
retry/reopen checks. Query output is synthetic; no real Slurm or relay is contacted.

Next: integrate prepared polling into the private bounded polling cycle and health
reporting/orchestration. Product event rendering/redaction, route revision semantics,
credential lookup, public review, licensing and activation remain pending.


## Subsequent bounded-cycle and health integration

Cycles::step_prepared now wraps the prepared scheduler path with the same durable
restoration, UTC window planning, budget accounting and health recording as plain
cycles. Separate fixture hooks provide query/preparation callbacks, explicit SMTP
subset and message-count limit; cycle destinations remain authoritative. Shared
step helpers keep plain-cycle behavior unchanged. Cancellation and exhaustion
precede restoration/query/preparation and do not consume budget; an attempted
failure consumes one cycle.

Static private health categories distinguish Preparation and PreparationLimit.
Invalid prepared settings map to existing InvalidRequest; scheduler, storage and
window failures retain their existing health categories. Failed preparation or
atomic persistence preserves the durable checkpoint, so health reports age/stale
status from the previous committed poll. A successful cycle resets the failure
streak and refreshes the checkpoint. Restart health uses the existing durable
checkpoint rather than assuming a recent poll succeeded.

Three tests cover prior checkpoint age/staleness after preparation failure,
reopened successful recovery without changing an earlier frozen payload, explicit
message limits before callbacks, cancellation/exhaustion, finite budget consumption,
invalid route/query/storage error categories, rollback and failure-streak reset.
The owned TLS submission fixture now starts with a prepared cycle (including
restore/window planning) before atomic commit, child send, refusal/lost-result
retry and reopen. No production entrypoint or public status contract is activated.

Synchronous fixture preparation still has count/message bounds rather than an
interruptible renderer deadline. Cancellation is checked before starting a cycle;
this increment does not interrupt in-flight query or preparation work. Schema,
dependencies, runtime defaults and public interfaces are unchanged.

Next: wire prepared cycle selection into private delivery/retention/health
orchestration, retaining existing cancellation and partial-progress behavior.
Event rendering/redaction, route revision, credentials, public review, licensing
and real-cluster/runtime activation remain pending.

### Private prepared SMTP orchestration

Prepared polling now selects the shared fallible tick orchestration. SMTP route
and message-limit policy is checked before clocks, queries, delivery or cleanup,
including exhausted polling. Existing capacity/storage/window preflight,
completion clocks, cancellation, partial-progress failures, cleanup and health
reporting use the same implementation as plain polling. Preparation failures allow
previously committed pending work to continue; storage/window failures stop
maintenance. Cancellation after commit leaves frozen pending messages intact.

Three new tests cover cancellation after prepared commit, frozen state on reopen,
exhausted-poll delivery/health, invalid-route rejection before side effects and
pending delivery after preparation failure with an unchanged checkpoint and
sanitized health failure. Existing orchestration failure tests exercise the shared
engine. No schema, dependencies, public interfaces or runtime defaults changed;
synchronous preparation remains bounded by count/size but is not interruptible.

Next: owned SMTP child acceptance through the complete prepared tick, including
retry/reopen and blocked-dispatch partial progress. Rendering/redaction, route
revision, credentials, licensing, public review and real-cluster activation remain
pending.

### Owned prepared SMTP tick acceptance

The owned frozen-submission fixtures now run the complete private prepared tick,
including atomic polling, delivery, retention and health. Temporary final refusal
and relay acceptance with a lost child result both produce a completion-based retry
at 25 after completion at 20. Reopening the owned store preserves the exact frozen
DATA and envelope, rejects changed content and suppresses duplicate preparation;
the retry succeeds through the verified loopback TLS child boundary.

A new three-destination prepared tick delivers the first route, then stops on a
missing helper for the second route. It preserves the first committed attempt,
reports a static spawn diagnostic without acceptance uncertainty and skips cleanup
and health. Reopen recovers the two pending routes with their original snapshots
and first attempt numbers, without resending the completed route. Final SQL state
contains exactly three delivery attempts.

Fixture-only secondary SQLite connections read committed snapshots while the
parent retains its owned-store guard. They close before child network calls. This
is acceptance plumbing, not a production storage/dispatch interface. Schema,
dependencies, runtime defaults and public contracts remain unchanged. Existing
missing/corrupt-snapshot rejection fixtures are retained.

Next: private SMTP event rendering/redaction design and bounded synthetic message
fixtures. Route revision, credential integration, licensing, public review and
real-cluster/relay activation remain pending.
