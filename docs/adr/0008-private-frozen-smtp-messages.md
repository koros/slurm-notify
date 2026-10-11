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
The subsequent private child experiment uses a separate seven-field frame: fixture
trust root, loopback port, username/password, saved sender/recipient and exact saved
DATA. The total input cap is 21,213 bytes, with per-field caps checked before
allocation and strict EOF. Credentials and DATA own zeroizing buffers. The original
ten-field renderer fixture remains separate. Child restoration validates addresses
and canonical DATA without rendering, adding headers, sampling a clock or restuffing
wire bytes; verified TLS/AUTH and existing bounded submission/acknowledgment policy
remain shared. This is private owned-snapshot transport, not a general raw email API.

A private load preflight maps missing, malformed or unreadable snapshots to blocked
local input before child construction. The worker records no attempt or completion
clock for this block. The caller resolves the snapshot before borrowing the store
for delivery; automatic production batch snapshot selection remains future work.
Owned-store fixtures prove temporary refusal and relay-accepted/lost-child-result
replay through reopen preserve the exact envelope, Date, Message-ID and DATA, even
when a conflicting newly rendered candidate exists. Real TLS fixtures compare every
observed DATA byte; malformed frozen child frames leave a listener unconnected.
Relay acceptance still permits duplicate delivery when the parent loses the result.

No dependencies, schema, runtime defaults or public interfaces changed in this
increment. Next: design atomic event/outbox/snapshot preparation, then event
rendering and route revision semantics. Public persistence review, credential
lookup, real relay/cluster validation, licensing and activation remain pending.


## Subsequent atomic preparation

[ADR 0010](0010-private-atomic-smtp-poll-preparation.md) adds a separate private
prepared poll path. Explicit new SMTP event/destination pairs commit their initial
snapshot in the same transaction as jobs, ledger, outbox and checkpoint. Historical
routes/messages and legacy missing snapshots are not backfilled by that path.
Standalone freeze_smtp remains a private legacy/fixture operation; production
scheduler preparation and runtime activation are still pending.
