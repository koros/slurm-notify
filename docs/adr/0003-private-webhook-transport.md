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
TLS uses the client's normal rustls/webpki roots; HTTPS certificate rejection and
enterprise CA behavior have not been tested here. No certificate bypass is added.

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
