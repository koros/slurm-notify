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
