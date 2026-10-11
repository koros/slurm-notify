# ADR 0014: private durable SMTP route revisions

Status: private rendering descriptors, atomic polling, guarded dispatch and cleanup implemented; durable provider profiles pending.
No public configuration, storage, routing or release contract is approved here.

## Problem and invariant

The private outbox key is (event_id, destination). Frozen SMTP messages preserve
sender, recipient and exact DATA, including Date and Message-ID. Changed rendering
policy does not rewrite pending snapshots, and replay does not enqueue historical
events for new destinations. However, destination names alone do not identify the
rendering policy that produced a snapshot, and transport is still supplied by the
fixture caller. A changed relay/authentication profile must never silently receive
a previously prepared delivery intended for another profile.

Bind each newly prepared SMTP outbox row to an immutable revision in the same
transaction as jobs, event ledger, outbox, frozen message and checkpoint. The
revision must describe the policy used for that preparation. Retries use the bound
revision and snapshot. Current configuration is used only to prepare newly emitted
events; it must not rewrite, rerender, relocate or backfill historical deliveries.
A failed poll leaves neither a new binding nor a new checkpoint published.

## First increment: rendering descriptor and fingerprint

Start with a pure private rendering descriptor, derived only from a validated
SMTP rendering plan. Include destination name, sender, recipient, namespace,
Hidden/Identifier policy, renderer version, identity-selection version and the
explicit private Message-ID domain. Exclude observed Date, event identity,
credentials, delivery attempts and operational limits such as batch size. Those
inputs either vary per event or do not change the rendering policy.

Use a versioned domain separator and typed length-prefixed fields, with stable
ASCII tags for enum/version values. Bound the canonical descriptor to 4,096 bytes
before hashing or persistence. Existing stricter bounds for route names, addresses
and namespaces still apply. Hash the complete canonical bytes with the already
locked ring SHA-256 implementation. Use 64 lowercase hexadecimal bytes as the
revision token. Input order of routes must not change a route's token. Semantically
changed fields must change the token; equivalent validated inputs must not depend
on allocation order, clocks or process-local hashing seeds.

Store canonical bytes as well as the token when durability is added. If an
existing token has different bytes, reject it as a conflict; do not overwrite it
or rely only on a digest comparison. Tokens are fingerprints, not encryption,
anonymity or authentication of the owned database. Descriptors contain private
addresses and must not implement Debug/Clone or appear in errors/logs. Diagnostics
use static categories. Existing user-owned SQLite permissions remain required.

This first descriptor covers rendering only. It is insufficient to select a
production SMTP relay or credentials. Existing loopback fixtures remain private
transport experiments and must not be presented as durable provider routing.

## Durable rendering binding

A later additive private migration will introduce immutable rendering revision
records keyed by (cluster, destination, revision), containing versioned canonical
bytes. A binding table keyed by (event_id, destination) will reference both the
outbox row and its revision. Validate cluster/destination coherence against the
event and descriptor inside the prepared poll transaction. Insert revision records
idempotently, reject descriptor conflicts, then insert the outbox snapshot/binding
and advance the checkpoint. Publish the candidate reconciler only after commit.
Do not modify migrations 0001/0002 or the existing outbox uniqueness rule.

A new revision does not add another delivery for an existing event/destination.
Different routing may fan out newly emitted events; already emitted or suppressed
events keep their ledger outcome, including after completed outbox cleanup. A
failed preparation may use updated policy when retried because it never committed.
An in-progress batch uses one immutable validated plan throughout preparation.

Preserve legacy rows and snapshots without inventing descriptor provenance. The
revision-aware dispatch path blocks a missing binding or unknown descriptor version
before child/network work and before recording a provider attempt. It does not
infer a revision from current configuration or silently backfill one. Existing
legacy fixture paths can remain available for testing migration and diagnostics;
production activation needs an explicit legacy recovery policy and public review.

Completed outbox cleanup may cascade its binding and frozen message together.
Revision cleanup must be separate, bounded and restricted to descriptors with no
remaining bindings, revalidated transactionally. Pending references prevent
pruning. Event/job deduplication history and checkpoints remain intact.

## Identity and dispatch policy

When revision-bound preparation is integrated, version the private Message-ID
selection algorithm and include the bound rendering revision token in its hashed
inputs for newly prepared deliveries. Keep observation time outside the ID and
freeze Date on commit. Existing snapshots keep their original version/bytes.
Public domain selection remains unresolved; fixture.invalid remains test-only.

Before production dispatch, add a versioned provider descriptor with explicit
relay host/port, implicit-TLS versus STARTTLS policy, TLS server name and trust
profile/content fingerprint, authentication identity and credential reference.
Require strict bounded validation and bind the provider descriptor durably too.
No passwords, tokens, private keys or credential values belong in either revision.
Resolving a reference must preserve its bound relay/TLS/auth identity; a missing or
incompatible profile blocks dispatch with a static error. Secret rotation under
an unchanged approved identity may resolve the current secret. Changing a relay,
trust policy or authentication identity requires a new provider revision.

Private revision-aware dispatch callers must explicitly provide the permitted
revision set; current destination names are not permission to retarget old work.
An old revision may drain only when it is permitted and its bound provider profile
is resolvable. Removed/disabled/unpermitted revisions remain pending and blocked
without fabricated attempts, cleanup or fallback to another provider. Decisions
to abandon or reroute pending work require a separate explicit operation and
future public contract review. No runtime default for that decision is introduced.
Cancellation and partial delivery progress retain the current orchestration rules.

## Implementation sequence and acceptance

1. Implement the bounded pure rendering descriptor/fingerprint from validated
   plans. Test a golden canonical encoding/digest, every policy-field change,
   equivalent route order, invalid inputs, bounds and static diagnostics.
2. Add the private additive migration and immutable revision/binding storage.
   Test schema upgrade, legacy preservation, token/bytes conflict, late-write
   rollback, missing references, cluster coherence and foreign-key constraints.
3. Connect binding to atomic prepared polling and versioned identity selection.
   Test changed policy on new events, unchanged prior snapshots, no historical
   backfill, suppression, failed-poll recovery and checkpoint/memory atomicity.
4. Add revision-aware controlled delivery and bounded unreferenced cleanup.
   Verify permitted/unpermitted revisions, legacy blocks, cancellation, partial
   progress, retry/lost-result restart and retained pending references.
5. Implement and verify durable provider-profile binding before production
   routing. Loopback tests must assert the exact bound profile, not merely changed
   ephemeral fixture endpoints. Credential integration and real relay/cluster
   validation remain separate gated work.

This milestone adds design only: schema version stays 2 and executable behavior,
dependencies, public interfaces, hooks and coverage gates are unchanged. The next
implementation milestone is sequence item 1, not the entire persistence/dispatch
plan. Licensing, product privacy defaults and public review remain explicit gates.

### Pure rendering descriptor implementation

The first private increment is implemented in smtp_revision. Descriptor::from_plan
accepts only an immutable validated Plan and an SMTP route lookup. It records
ordered destination, sender, recipient, namespace, disclosure, renderer version,
identity-selection version and Message-ID domain fields. Encoding begins with the
private version marker, then unsigned big-endian u16 tag lengths and u32 value
lengths followed by UTF-8 bytes. The complete 4,096-byte bound is checked before
allocation, copying or hashing. Canonical bytes use a zeroizing buffer; the
container omits Debug/Clone and exposes only borrowed bytes/token. Errors contain
static Route/Limit categories.

Existing renderer/identity versions are explicit private constants. The message
builder's fixture domain is centralized without changing generated messages.
Fingerprints use the already locked ring SHA-256 implementation; dependencies
remain unchanged. Tokens are exactly 64 lowercase hexadecimal bytes.

Five tests verify independent golden encoding/digest, changes to all route policy
fields and encoded version/domain inputs, equivalent route ordering and operational
limits, unknown/plain-route rejection, maximum validated address/name/namespace
inputs, invalid policy rejection and exact/over-limit encoding boundaries. Typed
lengths distinguish ambiguous tag/value concatenations. Descriptors exclude event
identity, observation time, credentials and operational limits.

Schema stays 2. No revision records, outbox bindings, versioned revision-aware
Message-IDs, provider selection, public contracts or runtime defaults are activated.
Next: additive private migration and immutable descriptor/binding storage, with
bounded canonical validation, legacy preservation, conflicts and rollback tests.
Atomic poll integration and durable provider profiles remain later increments.

### Private revision storage implementation

Additive migration 0003 advances private SQLite state to version 3. Immutable
rendering descriptors are keyed by cluster, destination and fingerprint; bindings
reference both the outbox/snapshot and descriptor. Insert-time cluster checks and
foreign keys reject incoherent references. Outbox removal cascades snapshots and
bindings; unreferenced descriptors remain for later bounded cleanup.

Explicit freeze_smtp_revision preparation atomically inserts the descriptor, new
snapshot and binding, or compares all existing values for idempotence. Conflicts
and late binding failures roll back every write. Existing unbound snapshots are
never assigned inferred provenance. Legacy polling/delivery remains unchanged.

Stored descriptors are bounded in SQL before materialization, decoded in fixed
field order, revalidated through Plan, and checked against the canonical SHA-256
fingerprint. Unknown versions, malformed fields, missing references and envelope
mismatches fail closed with static errors. Migration, reopen, immutable conflicts,
legacy preservation, malformed state, foreign keys and rollback have fixture tests.

Next: integrate these records into atomic prepared polling and versioned event
identity selection. Revision-aware dispatch, explicit revision permission and
durable relay/TLS/auth profiles remain subsequent work. No public contracts,
new dependencies, runtime defaults, real-cluster tests or releases are introduced.

### Atomic revision-bound polling and identity selection

Plan::poll_revision_with is an explicit private path that derives one immutable
v2-identity rendering descriptor per validated SMTP route before querying. It
prepares bounded messages with revision-aware Message-IDs, then commits jobs,
event ledger, outbox, snapshots, descriptors, bindings and checkpoint in the same
transaction. Candidate memory is published only after commit. Descriptor conflicts
and late binding/checkpoint failures roll back all new state. Plain destinations
receive no SMTP descriptor or binding. Revisions are inserted only for newly
prepared pairs; empty/suppressed/replayed polls leave no orphan descriptors.

The v2 identity hashes the existing length-prefixed event/route/namespace inputs
plus the exact rendering revision token under a new domain separator. Observation
time remains outside the ID and supplies the frozen Date. Descriptors explicitly
record sha256-event-route-revision-v2. Stored decoding accepts both known private
identity versions; legacy preparation and existing frozen bytes remain unchanged.
No schema migration, dependency or public contract change is needed.

Tests verify changed policy for new events, original bound bytes after reopen,
legacy and suppressed history without backfill, new destination fan-out, canonical
token/bytes conflicts, late binding/checkpoint rollback, recovery, rendering/count
failures and invalid revision preflight before scheduler work. An independently
computed v2 digest anchors identity tests; Date changes do not change identity.

Next: revision-aware controlled dispatch with explicit permitted revisions and
bounded unreferenced cleanup. Durable relay/TLS/auth profile binding remains
required before production routing. Runtime orchestration still uses the existing
legacy fixture path; no operational executable or real-cluster activation occurs.

### Revision-aware controlled delivery and bounded cleanup

The private smtp_revision_delivery worker requires a validated immutable allowlist
of (cluster, destination, rendering revision) entries. At most 64 entries are
accepted; an empty list denies all. Each due row loads and validates its durable
descriptor/snapshot before checking permission. Missing legacy bindings, malformed
state and denied revisions block with static diagnostics before sending, sampling
a completion clock or retiring exhausted retries. Approved sends receive the bound
descriptor and original snapshot, without consulting current rendering policy.

The shared controlled worker now accepts a checked authorization context before
budget retirement. Existing callers retain their prior behavior through a trivial
authorization adapter. Cancellation, partial committed progress, completion-based
retry scheduling and uncertain acceptance bookkeeping keep their existing rules.
Tests cover permission bounds, cluster/destination isolation, legacy/malformed
blocks, denied exhausted rows, partial progress, cancellation between sends, exact
retry bytes after reopen and completed snapshot/binding retention cleanup.

Store::prune_smtp_revisions removes at most 256 unreferenced descriptors for one
explicit cluster in an immediate transaction. Every binding protects its revision,
including pending and completed rows until ordinary outbox retention removes them.
A late-delete failure rolls back the complete cleanup batch. Tests verify bounds,
cluster isolation, late rollback, reopen and retained pending references. Jobs,
event history and checkpoints remain intact.

Owned loopback TLS acceptance exercises both temporary submission refusal and lost
child-result retry through this guard. Reopen with an empty allowlist blocks before
child work without another attempt; restoring the explicit permission delivers the
exact original snapshot and retains the two-attempt durable history. Relay/trust/
authentication profiles are still caller-selected fixture inputs: this guard is
rendering permission only, not durable production provider routing.

Next: design and implement durable provider-profile descriptors and bindings, then
verify exact relay/TLS/auth identity across restart. Public activation, credential
integration, privacy defaults, licensing and real-cluster/relay validation remain
separate gates. No new dependencies, schema versions or executable defaults change.
