# ADR 0007: private SMTP dependency assessment

Status: transport candidate deferred; test-only follow-up, no public contract.

## Decision and evidence

Do not add lettre's SMTP transport yet. Inspection of exact upstream tags finds
unmet gates from [ADR 0006](0006-private-smtp-boundary.md).

| Candidate | Declared compiler | Synchronous reply reader | Decision |
|---|---|---|---|
| 0.11.19 | Rust 1.74, edition 2021 | Accumulates replies through `read_line` without a byte cap | Defer |
| 0.11.23 | Rust 1.85, edition 2024 | Checks line and aggregate lengths after `read_line` returns | Defer |

Sources: tagged [0.11.19 manifest](https://github.com/lettre/lettre/blob/v0.11.19/Cargo.toml),
[0.11.19 connection](https://github.com/lettre/lettre/blob/v0.11.19/src/transport/smtp/client/connection.rs),
[0.11.23 manifest](https://github.com/lettre/lettre/blob/v0.11.23/Cargo.toml), and
[0.11.23 connection](https://github.com/lettre/lettre/blob/v0.11.23/src/transport/smtp/client/connection.rs).
Both connection implementations wrap a network stream in `BufReader`. A
continuously arriving unterminated line can grow the destination string before
newer checks execute. This source-based resource-bound finding is not a published
vulnerability claim or measured exploit. Parent deadlines bound elapsed time,
but do not establish a strict child heap bound. A compiler upgrade alone would
not resolve this allocation issue.

The [0.11.23 transport source](https://github.com/lettre/lettre/blob/v0.11.23/src/transport/smtp/transport.rs)
offers implicit TLS, required STARTTLS, explicit authentication mechanisms, and
socket timeouts. Required STARTTLS precedes authentication during connection
setup. Pooling must be disabled for the intended single-session child. Those API
capabilities still require our certificate, credential-ordering, deadline, and
final-acknowledgment acceptance fixtures. Socket timeouts do not replace the
parent's absolute deadline.

Only source and declared compiler requirements were assessed. Neither candidate's
resolved dependency graph was built here. Message validation, MIME encoding, dot
handling, backend certificate behavior, and internal retry/QUIT semantics remain
unverified. A message-builder-only dependency can be assessed independently.
No dependency, lockfile, toolchain, or operational default changes here.

## Next bounded experiment

Use the existing test-only rustls dependency and bounded reply reader to prove a
private implicit-TLS greeting/EHLO exchange over ephemeral loopback. Verify trusted
certificate success and unknown issuer, wrong hostname, and expiry failures before
any authentication or envelope command. Bound handshake and socket waits, discard
raw reply text, and confine trust overrides to fixture code.

This is a TLS boundary experiment, not a production SMTP client or a commitment
to implementing SMTP without a library. Required STARTTLS and supervised drip-fed
reply tests follow after that boundary passes. Reconsider a transport when bounded
reads become available or an explicitly reviewed isolation strategy supplies an
enforceable memory bound. Public activation, real relay testing, message and
persistence contracts, and licensing remain pending.

## Message-builder-only follow-up

The private construction experiment pins lettre 0.11.19 with defaults disabled and
only `builder` enabled. Its declared Rust 1.74 matches the development compiler;
local compilation and fixtures verify this resolved graph. SMTP transport, pooling,
TLS, hostname lookup, async, file/sendmail, and DKIM features remain disabled.
The tagged [message builder](https://github.com/lettre/lettre/blob/v0.11.19/src/message/mod.rs)
validates the envelope, derives it from sender/recipient headers, encodes headers,
and supplies Date when absent. The experiment always sets an explicit frozen Date
and Message-ID, preventing wall-clock/random defaults. The tagged
[body builder](https://github.com/lettre/lettre/blob/v0.11.19/src/message/body.rs)
normalizes String line endings and selects transfer encoding. The library's Address
parser validates ASCII addr-specs after our stricter size/character preflight.
It does not supply the SMTP DATA transparency used by our separate framing helper.

Resolved compatibility pins are email-encoding 0.4.1 (0.4.2 uses edition 2024),
idna_adapter 1.1.0 (1.2.2 uses edition 2024), and stacker 0.1.17/psm 0.1.24 to
retain the earlier stack-growth build dependency chain. The full locked graph is
compiled locally with Rust 1.74. These are test-only direct constraints, not a
production support or security certification. Lettre introduces the mailbox/parser,
encoding and stack-growth dependencies recorded in Cargo.lock; existing package
versions otherwise remain unchanged. Builder use does not reconsider the deferred
SMTP transport decision. Dependency security and licensing remain release gates.

Fixture bounds are explicit: addresses 254 bytes, local parts 64, stable identity
64, ASCII subject 128, printable ASCII/LF body 4,096, formatted and stuffed DATA
16,384 each. Source-sized inputs bound builder work; internal library allocations
are not independently heap-capped or zeroized. Our owned formatted/DATA buffers
are zeroizing; caller strings, Address and library Message copies are outside that
guarantee. No raw inputs appear in Error/Prepared diagnostics. Internationalized
addresses/subjects, MIME attachments, arbitrary headers, display names, quoted local
parts, address literals, recipient lists, and log forwarding remain out of scope.

DATA framing follows [RFC 5321 section 4.5.2](https://www.rfc-editor.org/rfc/rfc5321.html#section-4.5.2):
leading dots are doubled and exactly one terminator is appended. Every formatted
line is verified as bounded CRLF ASCII before framing. Message fields follow the
library builder and [RFC 5322](https://www.rfc-editor.org/rfc/rfc5322.html).
No connection, submission, relay acceptance, or durable frozen-message contract
is introduced by this increment.


## Subsequent security update

The earlier compiler assessment above is historical. ADR 0009 supersedes the
active builder pin with lettre 0.11.22 and development Rust 1.88.0. Builder-only
features remain enabled; SMTP transport remains deferred. The new builder uses
nom 8 instead of chumsky, allowing the obsolete stacker/psm constraints to be
removed. Existing explicit Date/Message-ID, ASCII input bounds, DATA framing,
frozen persistence and byte-observation acceptance continue to apply.
