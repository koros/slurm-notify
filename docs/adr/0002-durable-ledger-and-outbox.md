# ADR 0002: Durable ledger and per-destination outbox

Status: proposed design, with SQL schema experiment validated locally.
The schema is private and has no released compatibility guarantee.

## Context

The in-memory prototype loses observations and deduplication history on restart.
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
