# ADR 0015: private SMTP provider-profile revisions

Status: pure descriptors, trust fingerprints and atomic provider storage implemented;
provider-aware polling and resolved transport integration pending. This establishes no public configuration contract.

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
