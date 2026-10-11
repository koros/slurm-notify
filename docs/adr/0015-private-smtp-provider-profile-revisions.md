# ADR 0015: private SMTP provider-profile revisions

Status: pure descriptors, trust fingerprints and atomic provider storage implemented;
provider-aware polling and pair/resolution guards implemented; concrete bound transport pending. This establishes no public configuration contract.

## Identity and validation

Rendering revisions preserve message policy and bytes. Production routing also
needs immutable relay, TLS and authentication identity. A rendering permission
alone cannot authorize a changed relay or credential identity.

The private provider descriptor records, in fixed order: destination, relay host,
nonzero u16 port, implicit TLS or required STARTTLS, explicit TLS server name,
trust reference, trust-content fingerprint, authentication scheme (plain-v1),
authentication identity and credential reference. Ports have canonical decimal
encoding. Plaintext/optional TLS and implicit authentication defaults are absent.
Implicit TLS is a recorded profile option; the current authenticated submission
fixture implements required STARTTLS, so this is not evidence of authenticated
implicit-TLS delivery.

Names use bounded ASCII identifiers: destination up to 64 bytes, trust/credential
references up to 128. References are opaque identifiers, not file paths or lookup
instructions. Authentication identity uses the existing AUTH PLAIN bound of
1–64 printable ASCII bytes. Relay/server names are lowercase ASCII DNS names
(up to 253 bytes, 1–63-byte labels, no leading/trailing hyphens or trailing dot) or
canonical IPv4/IPv6 literals. URLs, ports in host fields, malformed numeric IPv4,
Unicode and alternate IP spelling are rejected. IDNA conversion is outside this
private boundary. Exact inputs are explicit policy, without endpoint defaults.

Trust fingerprints require a 64-byte lowercase SHA-256 token. The private trust
helper validates a caller-supplied DER root set through rustls RootCertStore,
then hashes a sorted, length-prefixed set under a separate version marker.
Duplicates and malformed/empty certificates are rejected. Bounds are 32 roots and
65,536 total DER bytes, checked before allocation or certificate parsing. Sorting
makes equivalent set order stable; changed DER content changes the fingerprint.
This parses trust anchors; it does not establish organizational trust policy or
verify a server certificate. System-root discovery and PEM loading remain outside
this experiment.

Provider canonical encoding starts with a version marker and uses the existing
private convention of u16 tag/u32 value byte lengths. Its full 4,096-byte bound
precedes encoding and hashing. SHA-256 covers all canonical bytes. Stored loading
bounds and decodes exactly ten ordered fields, validates known TLS/auth versions,
revalidates the profile and requires exact token and canonical-byte equality.
A provider descriptor has no Debug/Clone implementation; canonical bytes use a
zeroizing buffer and accessors borrow private metadata. Errors contain only static
categories. Tokens identify content; they do not encrypt or authenticate storage.

No credential values, private keys, resolved passwords, connection deadlines,
retry parameters or event observation times belong in this descriptor. Secret
rotation under an unchanged authentication identity/reference does not change the
provider revision. Changing an authentication identity, reference, trust set,
relay, TLS mode or server name does change it. Descriptor::matches compares both
canonical bytes and token against an explicitly resolved profile; it does not
perform resolution or send network traffic.

## Subsequent persistence and dispatch

1. Add private immutable provider records keyed by cluster/destination/token and
   per-outbox bindings alongside rendering bindings. Newly prepared pairs must
   commit both descriptors/bindings with snapshot, ledger and checkpoint. Preserve
   existing render-only and legacy snapshots without inferring provider provenance.
   Test migration, conflicts, rollback, missing references and cluster coherence.
2. Add explicit permission for the bound rendering/provider pair. Resolve trust
   and credentials using their bound references, require the exact fingerprint,
   relay, TLS mode/server name and authentication identity, then send the original
   snapshot. Missing/mismatched resolution blocks before child work, attempts or
   retry retirement. Secret rotation must retain identity and reference. Transport
   configuration must be built from the validated bound profile; comparing a token
   while using an unrelated caller transport would not satisfy this invariant.
3. Verify exact bound endpoints/trust/authentication in owned loopback TLS restart,
   temporary refusal and lost-result tests. Protect all referenced provider records
   during bounded cleanup. Cancelled/partial batches keep existing durable rules.

An old row may drain only with explicit permission and resolvable bound profiles.
Changing current settings must not relocate existing work. Backfill, abandonment
or rerouting requires a separately reviewed operation. Public contracts, credential
storage integration, privacy choices, license/support decisions and real-cluster/
relay validation remain gated. No release or production activation is implied.

## Validation

Pure tests cover an independent golden digest, stored round trip, all profile-field
changes and resolution mismatch, valid canonical DNS/IP names, rejected unsafe or
ambiguous inputs, maximum fields, overlong fields, truncation/corruption/unknown
versions, token mismatch, order-stable trust sets and root resource limits.
Existing locked ring/rustls/rcgen dependencies are reused without version changes.
Schema remains 3; existing rendering dispatch and the executable stay unchanged.

### Private provider storage and atomic preparation

Additive migration 0004 advances the private schema to 4. Provider descriptors
are keyed by cluster/destination/token; bindings reference the outbox and the
rendering binding with the same cluster. Foreign keys protect referenced provider
records and cascade bindings during completed outbox cleanup. Unreferenced
provider descriptors remain for a later bounded cleanup operation.

Store::persist_prepared_profiles requires a complete, unique provider descriptor
set matching the validated rendering destinations. It commits jobs, ledger,
outbox, frozen messages, both descriptor sets, both bindings and checkpoint in
one transaction. Existing token/bytes conflicts fail without overwriting records.
Only newly prepared pairs insert descriptors/bindings; replay does not backfill
legacy or render-only snapshots. Changed profiles apply to newly emitted events
and preserve existing profiles and exact message bytes after reopen.

Bound provider loading limits canonical BLOBs in SQL before materialization,
validates the fingerprint/profile and checks the cluster, destination, rendering
binding and snapshot. Malformed or missing referenced records fail closed. Tests
cover schema upgrades and failed migration recovery, immutable conflicts, route
coverage, late binding/checkpoint rollback, reopen, changed policy, missing/corrupt
records, foreign-key coherence and completed cleanup while preserving the ledger.

Next: connect immutable provider plans to scheduler polling, then enforce explicit
rendering/provider-pair permission and exact resolved relay/TLS/auth identity before
transport. This increment is the private atomic storage boundary; existing polling
and delivery adapters retain their prior fixture behavior. No new dependencies,
public contracts, executable activation or real-cluster/relay testing is introduced.

### Immutable provider-aware polling

Private ProviderPlan captures one validated provider descriptor and v2 rendering
descriptor per SMTP route before scheduler work. Its constructor rejects missing,
duplicate, unknown and invalid profiles; profile order does not select routing.
No Debug/Clone implementation exposes profile metadata. Polling validates Date,
uses the immutable route policy to render bounded messages and invokes the atomic
profile-storage boundary. Reconciliation memory is published only after commit.

The shared private preparation worker retains legacy/render-only adapters and now
has explicit profile settings with route coverage preflight. Provider-aware polls
commit both binding sets with messages, jobs, ledger and checkpoint. Plain routes
receive no SMTP profiles. Changed profiles apply only to new events; restored
history preserves its original profile and bytes. Suppressed and render-only
history does not receive inferred provider provenance on replay.

Four tests cover invalid/incomplete/duplicate profiles, reordered valid profiles,
new-policy selection with reopened old snapshots, deduplicated replay, late binding
rollback with unchanged memory/checkpoint and reopen recovery, invalid Date before
queries, rendering failure and suppressed/render-only history without backfill.
Existing Message-ID algorithms and delivery adapters are unchanged.

Next: validate explicit rendering/provider-pair permissions and resolve the exact
bound relay, trust and authentication identity before transport, with owned TLS
endpoint/restart acceptance. No new schema, dependencies, public contracts, runtime
defaults, installed monitoring or real-cluster/relay activation is introduced.

### Provider-pair permission and validated resolution guard

Private smtp_provider_delivery requires an immutable allowlist of at most 64
(cluster, destination, rendering revision, provider revision) entries. Empty lists
deny all; duplicate pairs, invalid names and malformed tokens are rejected. Each
due row loads both durable descriptors and checks the complete pair before local
resource resolution. Missing/corrupt bindings and denied pairs block before sending,
clock sampling or exhausted retry retirement. Existing fixture adapters remain.

Opaque Resolved construction compares the supplied profile to the bound descriptor,
hashes and validates the actual DER trust set, requires the exact authentication
identity, and validates credentials using the existing zeroizing AUTH PLAIN type.
No credentials are persisted or exposed through diagnostics. The worker compares
the returned context's canonical bytes/token with the stored provider again; a
resolver cannot substitute a context validated against another profile. Approved
sends receive the original snapshot and consume the validated profile/resources.
Secret rotation under the same reference/identity keeps the provider token stable.

Five tests cover pair validation and all matching fields, actual trust/auth identity
checks, secret rotation, missing legacy/render-only/corrupt bindings, denial before
resolution or retry retirement, resolution errors, wrong returned contexts,
cancellation and exact snapshot/pair replay across reopen with completion-based
retry scheduling. Diagnostics contain static categories only.

This increment validates caller-injected local resolution and guards an injected
send callback. It does not yet build a concrete network transport from Resolved;
an arbitrary callback is not proof of bound endpoint use. Next: construct verified
TLS/authenticated submission from the validated context and verify exact endpoint,
trust and authentication identity in owned loopback restart/lost-result acceptance.
Provider cleanup and orchestration integration follow separately. No new schema,
dependencies, public contracts, installed credentials/monitoring, real-cluster tests
or production activation are introduced.

### Concrete supervised bound SMTP submission

Private smtp_bound_transport consumes the validated resolution context and frozen
message through a bounded zeroizing stdin frame. The child revalidates canonical
provider identity, actual DER trust fingerprint, AUTH identity, envelope and DATA
before connecting to the bound relay/port. Required STARTTLS verifies the bound
server name against only the supplied trust roots, then authenticates and submits
the original bytes. Static acknowledgments use the existing retry classification.
Implicit TLS is rejected before spawning; no fallback selects another transport.

The parent supervisor bounds DNS, input, TLS, socket work and child output with an
absolute deadline. Child socket deadlines accept whole milliseconds from 1 to
30,000 and connection attempts inspect at most 16 resolved addresses. The helper
command remains a private trusted fixture choice, not an installed CLI contract.

Three unit tests cover frame round trips, every truncated frame prefix, trailing
bytes, field bounds, invalid deadlines and unsupported TLS before spawn. Owned
loopback acceptance keeps the original relay alive across SQLite reopen, verifies
TLS name and AUTH identity, denies changed settings before a decoy connection,
rotates the secret under the same identity and replays exact DATA after temporary
failure or a lost child result. Accepted sockets explicitly use blocking I/O.

Next: bounded unreferenced provider revision cleanup, then private orchestration
integration. No schema, dependency, public contract or production activation changes.

### Bounded SMTP provider revision cleanup

Private Store::prune_smtp_provider_revisions validates a nonempty cluster of at
most 4096 bytes and a deletion limit of 1..=256. One immediate transaction selects
unreferenced provider descriptors in destination/revision order, rechecks binding
absence on deletion and commits the actual removal count. Every existing binding
protects its descriptor, including pending and completed work awaiting retention.
The operation leaves rendering descriptors, snapshots, ledger and attempts intact.

Tests cover invalid bounds, bounded batches, cluster isolation, a late trigger
failure rolling back earlier deletions, pending protection and exact frozen bytes
after reopen. Completed-retention acceptance verifies cascading binding removal
makes the provider eligible for pruning and that deletion survives reopen without
removing event history. No schema, dependency or public contract changes occur;
cleanup remains caller-invoked with no new runtime default.

Next: private orchestration integration of provider-aware polling, pair permission,
validated resolution and supervised bound dispatch. Production activation and public
CLI/config contracts still require their separate review and acceptance gates.

### Private bound SMTP orchestration

Cycles::tick_bound_smtp composes provider-aware atomic polling with rendering/provider
pair permissions, validated caller-owned resolution and concrete supervised bound
STARTTLS submission. The existing stage engine now accepts a private delivery
policy; legacy ticks retain their delivery adapter. Poll validation follows initial
cancellation and precedes clocks/storage/queries, including exhausted cycles.
Profiles must cover all destinations: this increment deliberately supports only
SMTP routes. Timeout preflight requires whole milliseconds within 1..=30,000.

Existing stage ordering preserves durable poll state before sending, cancellation
between stages, monotonic completion clocks, partial delivery failures, retention
and health reporting. A blocked dispatch stops maintenance without inventing a
successful attempt. Helper command selection and secret/trust resolution stay
caller-owned; no environment lookup, installed helper, scheduling or cleanup default
is added. Provider revision pruning remains an explicit caller operation.

A network-free process fixture tests cancellation before validation and after poll
commit, invalid deadline preflight, denied pairs after reopen, exhausted polling
with pending delivery, temporary retry and final acceptance with unchanged frozen
bytes. Its synthetic acknowledgments exercise bookkeeping only. Owned TLS
acceptance now runs temporary/lost-result submission and reopened secret-rotation
recovery through the orchestration method, verifying the original endpoint, TLS
identity, AUTH identity and exact DATA. Changed profiles still fail before a decoy
connection. No schema, dependency or public contract changes occur.

Next: mixed-route dispatch policy and acceptance, preserving bound SMTP checks
alongside ordinary destinations. Public contract review, real-cluster policy
validation, installed monitoring and release gates remain outstanding.
