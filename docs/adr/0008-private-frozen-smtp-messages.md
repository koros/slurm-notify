# ADR 0008: private frozen SMTP envelope and DATA storage

Status: private test-only experiment; no released storage or email contract.

Persist one immutable validated sender, recipient and SMTP DATA frame per outbox
(event_id, destination), including the message's existing frozen Date/Message-ID.
Only the private message builder's Prepared type can create a snapshot. Preparation
commits in a separate immediate transaction before the first provider attempt;
network work must follow successful preparation. Atomic poll-to-render integration
is not implemented. An existing identical snapshot is idempotent, but changing any
bytes/envelope conflicts even after restart. New snapshots require pending status
and zero attempts; an attempted legacy row cannot fabricate a new message.

Migration 0002 adds smtp_messages and advances private user_version to 2. Version-1
outbox/attempt/checkpoint rows remain intact without inferred message content;
future versions are rejected. Each migration uses its own transaction: a fresh
file applies 0001 then 0002, so failure of 0002 leaves a valid version-1 file.
Upgrade failure rolls back version/table changes. Existing 0001 is unchanged;
its standalone Python fixtures continue to test that earlier schema. There is no
released downgrade guarantee. Migration alone does not activate SMTP delivery.

The snapshot foreign key cascades with removal of its outbox row under existing
retention operations. Pending rows retain snapshots because existing retention
protects pending outbox entries. No snapshot-only pruning or credential storage is
introduced. Stored message metadata/body are private user data in the already
owned SQLite state; this feature does not encrypt the database. Frozen payload
bounds are 254 bytes per address and 16,384 DATA bytes, checked by schema and loader.
SQL CASE bounds prevent oversized/corrupt fields being materialized into Rust.
Existing row comparison occurs inside SQLite rather than copying an unknown BLOB.

Loaded snapshots have no Debug/Clone, use zeroizing DATA buffers, and validate
address policy and canonical bounded DATA transparency without invoking a renderer.
This is validation of transport bytes, not cryptographic integrity or a general
MIME/header parser. Snapshot creation already required library-built messages;
malicious edits to an owned database are not authenticated by this scheme. SQLite
copies/pages, caller address strings, and upstream library copies are outside the
zeroization guarantee. Missing snapshots return None; malformed existing rows fail
rather than silently rebuilding or treating them as missing.

Four Rust tests cover exact byte/envelope preservation through retry/reopen,
immutable/idempotent writes, missing destination and attempted legacy conflicts,
version-1 upgrade/future-version guard, migration/insertion rollback, foreign-key
cascade, malformed DATA, address injection, and bypassed-check oversized BLOBs.
No dependencies or public interfaces changed. Next: compose frozen reads with the
submission child under a separate bounded frame; preserve bytes rather than
rebuilding from current configuration/templates. Event rendering, missing-snapshot
worker policy, route revision semantics, public persistence review, real relay/cluster
validation, licensing and activation remain pending.
