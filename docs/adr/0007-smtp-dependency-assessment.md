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
