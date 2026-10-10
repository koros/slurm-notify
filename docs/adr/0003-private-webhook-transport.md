# ADR 0003: private webhook transport experiment

Status: proposed, test-only; no public payload, configuration, or runtime contract.

## Decision

Use a blocking HTTP client behind a private test-only adapter. The caller supplies
serialized bytes, a stable printable event ID, explicit payload limit, request
timeout, and observation timestamp. POST uses application/json and an experimental
Idempotency-Key header. The `{}` fixture is not a normalized event schema.
Idempotency-Key alone does not guarantee deduplication by a receiver.

Endpoints must use HTTPS, except literal loopback HTTP addresses for local tests.
Reject URL user information and fragments. Authorization is optional and validated
against control characters. Reject oversized bodies and invalid IDs before sending.
Do not follow redirects or inherit environment proxies. These are prototype choices,
not approved deployment defaults; proxy and enterprise trust requirements need review.

A 2xx response is accepted. HTTP 408, 429, and 5xx are retryable; other statuses,
including redirects and authorization failures, are terminal. Transport errors
retry within the worker's attempt budget. Retry-After supports decimal seconds and
HTTP dates; past dates yield zero, while malformed or overflowing values fall back
to normal backoff. The worker preserves its backoff floor and persists the due time.

No response body is read. Endpoint/authorization structures intentionally have no
Debug implementation. Raw client errors and response content are not returned or
logged; diagnostics expose only static validation errors and provider outcomes.
The client parser bounds each header field to 100 KiB and the count to 100, which
is still larger than a future production response-header budget may allow.

## Dependencies and compatibility

All additions are dev-dependencies and absent from the normal executable:

- ureq 2.12.1: blocking HTTP with rustls certificate validation, avoiding an async
  runtime. Disable default features and enable TLS only; no gzip decompression.
- url 2.5.2: structured endpoint validation; pinned to avoid newer IDNA dependencies
  exceeding the development compiler baseline.
- httpdate 1.0.3: HTTP-date parsing for Retry-After.
- zeroize 1.8.1: explicit transitive TLS constraint; 1.9.1 uses edition 2024, which
  Cargo 1.74 cannot parse. The lockfile and Rust 1.74 suite verify this selection.

Compatibility pins are not a security assessment. Dependency maintenance, licenses,
current vulnerability checks, and the product support baseline remain release work.
TLS uses the client's normal rustls/webpki roots; Local HTTPS acceptance and certificate rejection are verified below;
enterprise CA behavior remains untested. No certificate bypass is added.

## Validation and limits

Seven new tests exercise exact POST bytes/headers, response classification,
Retry-After formats/fallback, redirect refusal, connection refusal, timeout,
validation before sending, and rate-limit persistence across database reopen with
stable IDs and terminal acknowledgment. Mock servers bind only loopback and need
no external network or Slurm access. Existing backoff tests verify attempt budgets.

Configured connect/read/write and overall request timeouts bound socket operations.
The client's synchronous DNS lookup cannot be interrupted and may exceed the
request timeout. This adapter therefore does not satisfy a strict production
wall-clock limit. Runtime activation must resolve that boundary before use.
Injected batch time also needs replacement with actual attempt completion time.
Payload schema, secure credential loading, jitter, routing, production concurrency,
HTTP-date edge policy, and public CLI/configuration still require further work.
The executable remains a placeholder and sends no notifications.

## HTTPS verification follow-up

Four loopback tests now verify acceptance with a matching IP and a fixture trust
root, and rejection of unknown issuers, mismatched names, and expired certificates
before HTTP data is received. Fixture trust is injected only inside the tests;
the adapter has no certificate-verification bypass or custom-root API. Certificates
and private keys are generated in memory and never committed. Accepted sockets
explicitly switch to blocking mode for portable TLS handshakes and retain read/write
timeouts. The server sends TLS close-notify and drains control records before exit.

Test-only rcgen 0.12.1 generates certificates. Direct rustls 0.23.45 selects the
same version already used by ureq for the loopback server and fixture trust store.
Time 0.3.36 and powerfmt 0.2.0 constrain rcgen's dependency graph for Rust 1.74;
powerfmt 0.2.1 requires Rust 1.79. Disable powerfmt's unnecessary default macros.
These additions do not affect the normal executable or its dependencies.

A fifth test injects a resolver that sleeps 100 ms into an agent with a 10 ms
request timeout. It returns a retry outcome only after the resolver finishes,
confirming the deadline limitation without external DNS or lingering threads.
Certificate errors currently share the bounded transport-retry outcome; targeted
TLS diagnostics and permanent-error classification remain future adapter work.

## Response-time HTTP-date evaluation

The clock-aware adapter samples time before sending and after receiving response
headers. HTTP-date Retry-After is converted to a delay from the response timestamp;
a slow send no longer includes elapsed request time in that delay. Missing/backward
response clocks fail visibly after the external request, preserving uncertain
acceptance. The injected-clock tests verify date arithmetic and regression. Relative
delay instructions continue to apply in full from parent attempt completion, so
IPC/cleanup latency may conservatively postpone a date-derived retry further.
The fixed-time send wrapper remains for deterministic existing fixtures.

## Sanitized diagnostic follow-up

The private adapter can now return an outcome plus a fixed diagnostic category:
HTTP status, DNS, connection, protocol, generic I/O, or configuration. It retains
no raw client error, source chain, endpoint, authorization, or remote body in that
report. Normal outcome-only callers and the private child protocol remain unchanged;
diagnostics are not yet persisted or exposed through a status CLI.

Invalid URL/scheme/HTTPS-policy/proxy configuration and proxy authorization error
kinds are terminal configuration failures. DNS, connection, protocol, and generic
I/O failures retry within the attempt budget. HTTP classifications remain unchanged.
The client's TLS certificate errors share its generic I/O kind and remain bounded
retries; do not guess certificate reasons from raw error strings. Granular TLS
classification and user-facing recovery guidance remain future work. Two new tests
verify category mapping and that rendered reports exclude endpoint, authorization,
and response content. There are no new dependencies or public diagnostic contracts.
