# ADR 0011: private bounded SMTP event rendering

Status: private test-only experiment; no public email format or privacy default.

Render lifecycle events before atomic prepared polling commits their frozen
messages. The renderer consumes the private lifecycle Event and explicit envelope,
message identity and timestamp metadata. It never samples a clock, generates an
identifier or reads configuration, credentials or logs. Caller metadata uses the
existing strict message builder validation and fixture Message-ID domain; stable
production identity/domain selection still requires design and public review.

Only Running and the eight terminal lifecycle states map to fixed ASCII labels.
Pending and Unknown are rejected with a static State category; raw unknown state
text is never copied into messages or diagnostics. Subjects contain fixed text and
that label. Bodies contain fixed explanatory text, state and a job field.

Every caller explicitly selects Hidden or Identifier; there is no Default.
Hidden emits [redacted] without inspecting or copying the job identity. Identifier
admits only a nonempty ASCII numeric allocation ID and optional numeric array
index, at most 32 bytes in total. Empty parts, multiple separators, controls,
Unicode and oversized values fail with a static Identifier category. Identity
validation precedes formatting; the renderer never silently truncates identifiers.
Cluster, submit and start context are always omitted. No job names, paths, user
names, raw records, log excerpts, severity inference or exit-code claims are added.
The private Event does not carry an exit code. This allowlist policy does not
implement arbitrary free-text secret detection or user-defined redaction rules.

Envelope addresses and explicit message metadata remain in normal SMTP headers.
Opting into an identifier discloses that identifier; this choice is test-only and
must not be interpreted as an approved runtime preference. Existing SMTP DATA and
body bounds still apply; this renderer's bounded fields produce messages below
1 KiB with synthetic envelope metadata. Error diagnostics contain only static
categories. Prepared and metadata types do not implement Debug or Clone. Frozen
DATA retains the existing zeroization behavior; this is not a guarantee that
upstream strings, library copies or SQLite pages are zeroized.

Tests cover all supported states and both disclosure choices, hostile hidden
identity fields, deterministic bytes/envelope, valid identifier size boundaries,
injection/Unicode/malformed identifiers, unsupported states and invalid envelope,
identity and timestamp metadata. A prepared-cycle test verifies renderer failure
leaves no ledger/outbox/checkpoint after reopen, followed by successful redacted
preparation, health recovery and exact frozen bytes on a second reopen. Existing
prepared cycle fixtures now use the renderer. The owned TLS partial-progress
fixture also renders before atomic commit and reuses snapshots after restart,
without invoking the renderer for duplicate events.

No schema, dependencies, runtime defaults, public contracts or executable behavior
changed. Route revision and production credential/identity integration remain
pending. Next: exercise disclosure selection and rendering failure in a mixed
SMTP-route atomic poll, including later-route rollback and restart recovery.
