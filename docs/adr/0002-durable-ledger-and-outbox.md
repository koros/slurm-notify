# ADR 0002: Durable ledger and per-destination outbox

Status: proposed design, with SQL schema experiment validated locally.
The schema is private and has no released compatibility guarantee.

## Context

The initial in-memory prototype lost observations and deduplication history on restart.
The handoff selects user-local SQLite and requires event recording and enqueueing
in one transaction. External acknowledgement cannot provide exactly-once delivery.

## Proposed transaction boundaries

One storage owner serializes writes. Enable and verify foreign keys on every
connection before starting transactions; apply migrations transactionally and
reject newer database versions without changing them. Do not silently replace or
reset unreadable state. Bound lock waits and surface disk-full/busy/corruption errors.

For a successful poll, calculate the candidate reconciliation state, then commit
job snapshots, event identities, all routed destination records, and the historical
checkpoint together. Only after commit should the in-memory candidate replace the
previous state. Failed queries, parsing, routing, or storage must leave both the
checkpoint and reconciliation state unchanged. Suppressed initial events need
persistent deduplication markers but no destination records. Unique logical event
keys detect repeated observations even if a caller proposes another event ID;
conflicting payload/identity data must fail visibly rather than be silently ignored.

For delivery, read committed pending records, send outside database transactions,
then atomically record the attempt and resulting pending/delivered/failed state.
Store credential-free destination identifiers only. A crash after a provider accepts
an event but before the local commit may cause a retry; retain stable event IDs for
provider deduplication where available. The schema experiment assumes one delivery
owner and does not implement multi-worker claiming or leases.

## Prototype schema and tests

`crates/notify-storage/migrations/0001.sql` reserves jobs, events, outbox,
delivery attempts, and cluster checkpoints, with uniqueness and foreign-key
constraints. Empty started_at denotes a pre-start identity. Observed/attempt times
are nonnegative integer placeholders; their precise unit must be defined before
Rust integration. No secrets, full logs, or external event payload schema is stored.

Python standard-library SQLite tests exercise commit/reopen, rollback on failed
fan-out, uncommitted connection close, logical event/destination uniqueness,
foreign-key protection, delivery outcome atomicity, and migration failure/version
handling. These tests validate SQL behaviour, not Rust persistence, real process
crashes, power-loss durability, or complete reconciler restoration.

## Retention and migration policy still required

Do not delete queued/retrying deliveries. Preserve event deduplication markers for
at least the supported catch-up/restart window before pruning resolved records.
Prune children and parents transactionally only under an explicit approved policy;
the prototype uses RESTRICT to prevent accidental orphaning. Retention durations,
queue limits, disk limits, migration compatibility, rollback policy, and journal
mode remain undecided. There is no destructive migration or cleanup implementation.

Database files/directories must be user-owned and restrictive. Atomic creation,
symlink handling, journal file permissions, network-filesystem support, backups,
and integrity diagnostics require implementation and tests. No installation path
is established here.

## Dependency assessment before implementation

Recommend assessing rusqlite first for a synchronous, single-owner SQLite layer;
its transaction API fits this small local workload. Compare sqlx if runtime/query
requirements justify the additional async/dependency footprint. The CLI has no
async runtime yet. No Rust dependency is added by this experiment. Before adopting
a version, verify MSRV against the development toolchain, dependency licenses and
advisories, bundled versus system SQLite, native build requirements, target support,
and binary size. Do not infer a production support baseline from local Python SQLite.

References: https://www.sqlite.org/foreignkeys.html and
https://docs.rs/rusqlite/latest/rusqlite/.

## Prototype dependency and Rust operations

Adopted `rusqlite = 0.31.0` with default features disabled and `bundled` enabled,
strictly as a dev-dependency for the test-only private storage module. Its
synchronous transactions suit the existing single-owner prototype without adding
an async runtime. Version 0.32.1 failed to compile on Rust 1.74 due to C-string
literal syntax; 0.31.0 and the locked transitive graph compile locally. Bundling
avoids a system SQLite installation but adds a native C build requirement for
developers/CI. Normal executable builds do not link this dev-dependency, so no
production binary footprint or target support conclusion follows from these tests.

Cargo metadata reports MIT for rusqlite/libsqlite3-sys and MIT OR Apache-2.0 for
selected hashing/build dependencies; this is a metadata assessment, not a complete
license audit. No security advisory audit or release approval is claimed. This
older SQLite bundle is a prototype choice and must be reassessed against current
advisories, release requirements, and the final toolchain before production use.
Versioned upstream source: https://docs.rs/crate/rusqlite/0.31.0/source/Cargo.toml.

Private Rust operations enable foreign keys, use caller-supplied lock timeouts,
apply the initial migration, reject unsupported versions, and atomically persist
job snapshots/events/fan-out/checkpoints. Identical logical event replay keeps the
original delivery routes; a different ID for the same logical event is a conflict.
Observed timestamps in this prototype use Unix seconds. No payload or secrets are
stored. Checkpoint ordering and job-state monotonicity must be ensured by the
future reconciler integration; these operations do not establish those guarantees.

Six Rust tests cover commit/reopen/replay, fan-out rollback, ID conflicts, newer
schema rejection, bounded lock contention, and invalid/orphan input. Store opening
currently assumes a private caller-owned test directory. Secure file creation,
full schema verification, migration upgrades, pending-delivery reads, delivery
outcome operations, state restoration, and actual process-crash tests remain pending.

## Integrated candidate commits and bounded restoration

The private mock-polling pipeline now clones the current reconciler, applies a
successful snapshot to that candidate, commits its snapshots/ledger/fan-out and
checkpoint, and only then replaces the live state. Query, parse, or storage failure
preserves both live state and the existing checkpoint. Checkpoints cannot regress;
fixed-width UTC query-window validation makes their lexical comparison ordered.
Generic storage callers must also provide ordered checkpoint representations.

Internal event IDs encode the cluster/job/submission/start/state tuple with explicit
UTF-8 byte lengths. They are deterministic and unambiguous without hashing, but
are not a public event contract or a privacy-preserving hash. The event ledger
includes suppressed initial events without outbox destinations. Replays preserve
the original routes, including a decision not to notify an initially suppressed
event. No notification payload or complete logs are stored by this prototype.

Restoration reads a consistent transaction snapshot, applies separate row limits
to jobs and ledger entries, validates event IDs and job references, and restores
initial-event suppression and terminal-state deduplication. Capacity or consistency
failure leaves the database intact and returns an error. Restoration returns the
checkpoint; it does not choose a catch-up window or polling interval automatically.
The checkpoint records the last successful query window, not proof that accounting
is complete through that time; future catch-up must allow overlapping late records.
Historical actual attempts remain recorded. When a pending job gains an actual
start, its obsolete pending row is removed atomically so reopening cannot recreate
that provisional identity. Only pending state may be promoted; a terminal pre-start
cancellation is not mistaken for an observed pending job.

Eight added Rust tests cover deterministic identity encoding, full lifecycle reopen,
pending promotion, suppressed events, failed commits/query recovery, corrupt IDs,
restore capacity, and checkpoint regression. There are now 46 passing Rust tests.
These are local database close/reopen tests, not proof of power-loss durability,
real cluster semantics, multi-writer coordination, or external provider delivery.

Secure production database creation, hard peak-memory/retention policies, schema
upgrade compatibility, delivery outcome operations, runtime activation, and a
production-ready SQLite dependency/toolchain assessment remain pending.

## Committed outbox and mock delivery worker

The private worker reads a caller-limited batch of due pending records for one
cluster in deterministic due/event/destination order. It validates event identity
and reconstructs the minimal lifecycle record without reading Slurm or logs.
Only committed rows are visible. Provider callbacks execute outside storage
transactions; a separate connection can acquire a write transaction during a send.

A result commits the attempt number/outcome and the outbox status/schedule in one
transaction. A stale attempt fails visibly without adding a second attempt or
changing status. Success and terminal rejection stop delivery to that destination;
transient failures retry until the caller-specified budget is exhausted. Reducing
the budget retires already-exhausted pending records without fabricating a send.
Provider rejection is isolated, but a storage failure stops the batch so accepted
and unrecorded sends cannot be silently treated as complete. Prior successful
commits within that batch remain committed.

The test-only retry policy requires explicit maximum attempts, positive initial
delay, and a maximum backoff. Exponential delay is capped and integer-overflow
checked before sending. There are no product defaults, jitter, HTTP Retry-After header parsing,
transport timeouts, payload formatting, or real provider configuration yet.
Observed/attempt times are injected Unix seconds. A production worker must use
actual attempt completion time and handle provider rate-limit instructions.

One delivery owner is required. Due reads are not claims or leases and do not
prevent two workers sending the same row concurrently. External acceptance before
a failed local result commit can cause replay with the same stable event ID;
provider acknowledgement does not imply human receipt. Tests exercise this
at-least-once limitation explicitly. No exactly-once guarantee is made.

Eight added tests cover delivery/reopen, timed retries, provider isolation, batch
bounds/retry exhaustion, reduced budgets, failed outcome transactions, stale
attempts, and invalid/overflowing policy. There are now 54 passing Rust tests.
The worker remains test-only; no network requests, email, or actual notifications
are performed. Secure runtime activation and real provider integration remain pending.

## Provider-requested retry delay

The injected provider may return a non-negative relative delay in seconds. The
worker schedules the greater of that delay and its exponential backoff. The
backoff cap limits exponential growth; it does not shorten a provider instruction.
The resulting due time is committed with the attempt and survives reopen. The
attempt budget still applies, so a final transient failure becomes failed even
when the provider requests a longer delay.

Negative delays or due-time overflow stop the batch visibly before committing
that result. The external send may already have occurred; the pending attempt
can replay with its original event ID. Earlier results remain committed. This
uses the injected batch timestamp; real transport must provide accurate completion
time. HTTP delay/date parsing and malformed-header fallback remain adapter work.
Three additional tests cover restart, backoff floors, exhaustion, and invalid hints.

## Completion-clock scheduling follow-up

The private worker now accepts a completion-clock callback and samples it after
each provider callback. It records that completion timestamp in attempted_at and
adds the backoff/provider delay to it. Slow destinations therefore receive their
full retry delay after completion instead of an already elapsed batch deadline.
Due selection still uses the initial batch timestamp; jobs that become due during
the batch wait until the next batch. Earlier fixture tests use a fixed-clock wrapper.

Missing, negative, or backward completion readings stop the batch without committing
that outcome. Regression means earlier than the batch timestamp or the preceding
completion within that batch. Prior successful outcomes stay committed. An accepted
but unrecorded delivery can replay with the same ID. Due-time arithmetic remains
checked, including overflow after a large forward clock jump. Forward jumps advance
stored timestamps; no synthetic correction or monotonic-to-wall conversion is added.
Wall-clock changes across restart can still postpone or accelerate due selection.
