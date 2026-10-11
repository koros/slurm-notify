# ADR 0013: private per-event SMTP identity and frozen Date

Status: private test-only experiment; no released Message-ID/domain contract.

Validated rendering routes now carry sender, recipient and an explicit namespace,
with no route-level timestamp or literal message identity. Namespace validation
uses the existing strict builder probe (1..64 ASCII alphanumeric/hyphen/underscore
bytes). Namespaces may be shared because destination names participate in identity
selection. Construction no longer requires distinct literal route Message-IDs.
The low-level arbitrary fixture renderer/builder still accepts explicit metadata.

A private selector hashes cluster, allocation/array job ID, submission context,
actual start context, lifecycle state, destination name and namespace with SHA-256.
The digest starts with a versioned private domain marker; each UTF-8 field has an
unsigned 64-bit big-endian byte length prefix. Boundaries are unambiguous and
output is 64 lowercase hexadecimal bytes accepted by the existing message builder.
All fields are limited to 4,096 bytes before hashing; empty start context is allowed
for cancellation before execution. Other identity fields must be nonempty.

The digest avoids placing raw cluster/attempt data into headers. It is a stable
fingerprint, not encryption or an anonymity guarantee: guessable inputs can be
matched offline. Collision resistance is cryptographic best effort, not a claim
of mathematically unique IDs. This is separate from SQLite's existing private
length-prefixed event ID and does not change the event/storage schema. Sender,
recipient and disclosure are not hashed; durable route revision remains pending.
The builder retains its private fixture.invalid Message-ID domain.

Date uses the caller's observed_at time from the prepared poll, validated from
Unix epoch through 2099. It represents observation/preparation, not scheduler
occurrence time. No clock is sampled by rendering/selection. Observation time is
excluded from the identity digest so retries do not get new identities. Atomic
prepared storage freezes the exact Date, Message-ID, envelope and DATA on commit;
replay skips selection/rendering and delivery always uses the stored snapshot.
A failed preparation can choose a later Date when it eventually commits because
no prior message was durably prepared. Invalid Date ranges fail before querying.

Dependency: add a direct private dev dependency on ring 0.17.14, already locked
and used by the TLS fixtures. Its incremental SHA-256 API avoids custom crypto and
unbounded concatenation of identity fields. Only the root lockfile dependency edge
changes; registry package versions and production dependencies remain unchanged.

Tests include an independent SHA-256 golden vector, variations of all event
identity fields/state, destination and namespace, ambiguous field segmentation,
changed observation time, cancellation without a start context and byte/date
bounds. A mixed two-event/two-SMTP-route fixture verifies four distinct actual
Message-ID headers and exact observed Date headers; reopening and polling at a
later time preserves all frozen bytes and the six mixed-route outbox entries.
Existing validated tick and owned TLS partial-progress restart fixtures now use
selected event metadata rather than reused literal IDs/dates.

No schema, public interfaces or executable defaults change. Next: owned TLS retry
and lost-child-result acceptance using selected per-event metadata end to end.
Production Message-ID domain, durable route revisions, credential integration,
public review, privacy defaults and real-cluster/relay activation remain pending.
