# 0063: Transactional public webhook identity, bytes and route revisions

Date: 2026-10-23
Status: accepted internal local-MVP implementation; no installed monitoring activation

## Decision

Preserve private internal deduplication IDs. Generate independent UUID-v4 public IDs
from the existing ring SystemRandom (16 bytes, version/variant normalization), retrying
at most eight collisions under the immediate SQLite transaction. Add explicit public
webhook persistence and an opt-in schema-5 transaction body. Existing private writer
paths stay at version 4; reopening schema 5 is supported. Historical inspection reads
both without migration. Old rows are not fabricated into public events.

New routed events atomically store public UUID, frozen minimal JSON, SHA-256 integrity,
route revision bindings, original ledger/outbox/jobs and checkpoint. Deferred route-to-
frame foreign keys permit route insertion before the parent frame seals the set.
Triggers prohibit later frame/route mutation/deletion/addition. Retention of outbox
rows cannot erase the frozen event's selected set. Caller reconciliation advances only
after commit, preserving recovery after entropy, collision, validation or SQL failure.

Compose prepared webhook resources with selected public persistence. Bind exact
resolved endpoint bytes and URL/auth reference names with the shared versioned hash
codec; omit resolved credential values and plaintext endpoints from storage. Require
an exact current revision on delivery lookup. Public composition rejects HTTP and
incomplete/mixed provider sets before query, checks selected provider payload bounds,
and cannot silently omit SMTP routes. Mixed SMTP atomic composition remains separate.

Serialize the adopted sender shape, canonical allocation/task decimal strings and
explicit state/severity mapping. No numeric narrowing or guessed Slurm width. Bound
JSON bytes to 16,384 and observed timestamps to nonnegative supported UTC seconds.
Source occurrence/submission/start timestamps stay null until independently established;
never export private attempt context or fabricate source time. Frozen retrieval checks
bounded columns, checksum, UUID and JSON/public-ID agreement before returning bytes.

## Evidence

Tests cover all lifecycle mappings, allowlisted fields, v4/variant bits, synthetic
contract example, decimal/range/step rejection, invalid timestamps, both successful
collision retry and exhaustion, entropy failure, late SQL rollback including DDL,
unchanged caller reconciliation, suppressed/unmatched/legacy events, immutable
route/frame SQL, retry/restart byte identity and changed endpoint/revision blocking.
Corrupt hashes/JSON/ID agreement fail closed. Public HTTP, incomplete provider sets
and oversized payload admission fail without advancing state. Installed status tests
inspect both schema versions byte-for-byte unchanged. Shared checks remain required.

## Remaining work

Bind this internal path into mixed SMTP preparation, supervised delivery and owned
continuous sessions. Old private pending records have no public frame and need an
explicit activation/reconciliation policy, not automatic backfill or discard. Add
installed monitoring/restart acceptance before claiming notifications work. No cluster,
real provider request, service installation or release is authorized by this increment.
