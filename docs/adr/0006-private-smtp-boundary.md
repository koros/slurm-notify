# ADR 0006: private SMTP transport boundary and acceptance plan

Status: proposed, test-only; implementation and public interfaces are pending.

## Scope

Add one private SMTP submission adapter behind the existing supervised child and
fallible delivery worker. It submits to one explicitly configured relay rather
than discovering MX records or running a mail server. No CLI, configuration format,
message template, public email schema, port, timeout, or authentication default is
approved here. Email remains required MVP work in the design handoff.

One durable destination corresponds to one recipient and one frozen message.
Multiple recipients must become separate destinations before sending, avoiding
partial recipient acceptance hidden behind one outbox outcome. Recipient expansion,
route revisions, and frozen message bytes require later persistence design.
The initial experiment uses synthetic fixtures and a single recipient.

## Input and privacy boundary

The caller supplies relay hostname/port, TLS mode, optional explicit authentication,
envelope sender/recipient, bounded message bytes, stable private message identity,
and socket/parent deadlines. Validate all limits before spawning. A bounded frame
carries request data through stdin; arguments contain only trusted helper mode.
The existing five-field HTTP frame cannot silently become an SMTP contract: use a
separate private mode and strict decoder with field/total limits and EOF checks.

Reject control characters and header injection in structured fields. Use a mail
library's mailbox/message validation rather than concatenating untrusted protocol
commands. The first fixture scope excludes SMTPUTF8 addresses, attachments,
arbitrary custom headers, multiple recipients, and user log content. MIME encoding,
line length, dot handling, and Unicode subjects must be delegated to a validated
message builder and covered by fixtures before broader support.

Request/authentication types have no Debug output. Raw SMTP reply text, relay
addresses, usernames, mailbox addresses, message bodies, and credentials must not
appear in parent diagnostics, database attempt records, or test-failure output.
Use synthetic placeholder credentials only. Do not inherit ambient credential,
proxy, relay, or TLS settings. Secret source lookup happens before dispatch;
missing credentials block locally without a fabricated provider attempt.

## TLS and authentication

Prototype modes are explicit implicit TLS or required STARTTLS. There is no
opportunistic downgrade. Validate trust, expiry, and relay hostname before AUTH
or message submission. Missing STARTTLS or failed TLS negotiation stops before
credentials or message data are sent. Loopback plaintext is confined to isolated
unauthenticated parser fixtures; it is not an operational mode.

After STARTTLS, discard pre-TLS capabilities and obtain a fresh EHLO capability
set, as required by [RFC 3207](https://www.rfc-editor.org/rfc/rfc3207.html).
The first authenticated experiment may use explicitly selected PLAIN only inside
verified TLS. Unsupported advertised mechanisms block rather than falling back
to another scheme. OAuth, client certificates, enterprise CAs, unauthenticated
trusted relay support, and secret-store integration remain review decisions.
Test trust overrides must stay confined to in-memory fixture infrastructure.

## Acknowledgment and failure policy

SMTP transaction completion is distinct from mailbox delivery. A positive final
reply after DATA means the relay accepted responsibility, not that a researcher
received or read the message. Temporary and permanent reply classes are described
in [RFC 5321](https://www.rfc-editor.org/rfc/rfc5321.html), sections 4.2 and 4.2.5.
These references explain protocol semantics; the policies below are prototype
choices rather than SMTP-mandated retry defaults.

| Evidence | Private worker action | Diagnostic / acceptance |
|---|---|---|
| Final positive completion after DATA | Delivered | RelayAccepted / accepted by relay |
| Relevant 4xx reply before or after DATA | Retry within existing budget | TemporaryReply / explicit nonacceptance |
| Relevant 5xx envelope or DATA rejection | Rejected | PermanentReply / explicit nonacceptance |
| Authentication 5xx | Block dispatch for credential correction | Authentication / not accepted by relay |
| Required TLS absent or invalid certificate | Block dispatch | TlsPolicy / message not submitted |
| Missing/invalid local input or helper spawn failure | Block dispatch | Configuration / not attempted |
| Disconnect/timeout with no final acknowledgment | Retry within budget | ConnectionOrTimeout / unknown acceptance |
| Malformed reply, helper exit, or parent cleanup failure | Retry within budget | ProtocolOrChild / conservatively unknown |

Track transaction phase privately to avoid claiming not-attempted merely because
submission failed after network contact. Credential/TLS blocks may involve a
connection but must establish that no message submission occurred. A final success
must not be reversed by a later QUIT failure. Child acknowledgment or outcome-commit
loss still leaves uncertainty at the parent boundary. The existing generic
classifier has only HTTP-era acceptance categories; extend private diagnostics
explicitly before mapping SMTP blocks into it.

Stable Message-ID is useful for correlation and must survive retries; SMTP does
not provide the webhook-style idempotency guarantee. An accepted message can be
submitted again after acknowledgment/commit loss. Keep at-least-once semantics
visible and do not promise recipient deduplication. There is no SMTP Retry-After
header policy; use the existing explicit backoff, jitter, and attempt budget.

## Resource and execution limits

Each attempt opens one session for one message/recipient and then exits. No pool,
pipelining, automatic provider retry, or background queue belongs inside the child.
Socket connect/read/write limits complement the parent's monotonic deadline.
DNS blocking remains contained by process supervision. Bound request/message bytes,
reply line/aggregate bytes, capability count, and child stdout. Library parser
limits must be inspected; parent stdout limits do not bound SMTP response memory.
A drip-fed reply must not extend the parent's deadline indefinitely.

The parent retains ownership and performs no network work in a SQLite transaction.
Cancellation finishes current acknowledgment/bookkeeping before stopping later
destinations. Blocked local dispatch leaves the pending row and attempt unchanged;
transient attempts use completion-time scheduling. Use the existing sanitized
partial progress reports for failure and cancellation.

## Dependency gate and implementation order

The handoff suggests lettre; treat it as a candidate, not an approved dependency.
The initial [dependency assessment](0007-smtp-dependency-assessment.md) defers its
transport because the inspected versions do not meet every gate below.
Before selecting a version, inspect official API/source for Rust 1.74 compatibility,
TLS trust/hostname behavior, explicit STARTTLS enforcement, authentication control,
message validation, internal retry behavior, timeouts, and reply parser limits.
Assess transitive dependency compatibility and explain any pins. Development
compatibility is not a security or product support certification.

1. Implement private transaction-phase/outcome classification with no new dependency.
2. Build bounded loopback fixtures for replies, slow/disconnected sessions, and
   message observation, without external SMTP accounts or Slurm.
3. Select a test-only client/message dependency and verify strict TLS/auth ordering.
4. Connect the separate SMTP request mode to the trusted child and parent deadline.
5. Compose owned storage, retries, cancellation, blocked dispatch, and restart tests.
6. Review public email/configuration/persistence decisions before runtime activation.

## Acceptance matrix

| Boundary | Required evidence |
|---|---|
| Inputs | Invalid addresses/control characters/oversize frames fail before connection |
| Protocol | Multiline replies, malformed/truncated replies, bounded capability data |
| Submission | Success only after final DATA acknowledgment; QUIT failure cannot undo it |
| Rejection | Envelope, recipient, DATA, and authentication 4xx/5xx classified by phase |
| TLS | Trusted fixture succeeds; wrong name, expiry, unknown issuer fail before AUTH |
| STARTTLS | Missing extension/refused upgrade blocks; new EHLO follows verified handshake |
| Credentials | Placeholder AUTH occurs only after TLS; credentials never appear in diagnostics |
| Deadline | Blocked DNS, slow greeting, drip-fed replies, slow DATA acknowledgment bounded by parent |
| Restart | Retry deadline, attempt number, message identity/body persist after reopen |
| Uncertainty | Accepted message with missing child acknowledgment or failed DB commit can replay |
| Isolation | One failed recipient cannot erase prior destination commits |
| Cancellation | Earlier delivery retained; later destinations keep original attempts |

All ordinary acceptance uses ephemeral loopback servers and temporary owned stores.
Real relay tests require separate user authorization and site policy validation.
Releases, licensing, support baselines, and operational onboarding remain unresolved.
