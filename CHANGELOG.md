# Changelog

All notable changes will be documented here. Versioning and compatibility follow
the design handoff; no release has been published.

## [Unreleased]

### Added

- Initial folder scaffold and project README.
- Private, dependency-free Rust executable workspace with development toolchain pin.
- Foundation plan, architecture decision, contributor and agent guidance.
- Basic Linux CI for formatting, linting, tests, build, and placeholder smoke check.
- Contract review guidance; no public contract baselines have been approved.
- Test-only accounting parser and lifecycle reconciler with eight fixture-based tests.
- Test-only active-job parser and bounded direct-process runner with failure, timeout, output-limit, and argument-safety coverage.
- Provisional pending-to-attempt identity promotion and atomic active/accounting snapshot reconciliation, with delayed-accounting and reused-ID tests.
- Test-only pre-start cancellation reconciliation with explicit catch-up policy and atomic rejection of ambiguous attempts.
- Linux/macOS process-group cleanup, bounded reader cleanup, and scheduler formatting-environment controls with subprocess tests.
- Connected injected scheduler queries to atomic parsing and lifecycle reconciliation, with explicit accounting windows and source-specific failure tests.
- Strict accounting-window calendar validation, node/boot/deadline terminal-state coverage, and atomic retained-state capacity checks.
- Proposed SQLite ledger/outbox ADR, private migration experiment, and eight transaction/reopen/migration tests with separate CI execution.
- Private Rust SQLite poll transactions and six rollback/reopen/conflict/lock tests, using a pinned test-only rusqlite dependency.
- Version-controlled pre-commit/pre-push hooks, isolated staged verification, staged-file guardrails with six tests, and a shared local/CI check script.
- Connected mock polling to atomic SQLite commits and bounded reconciliation restoration, with stable internal event IDs and restart/rollback tests.
- Prevented durable checkpoint regression and removed obsolete pending identities during actual-attempt promotion.
- Bounded committed-outbox reads and atomic delivery outcomes with stale-attempt protection.
- Mock provider worker with explicit capped backoff, retry budgets, per-destination isolation, and delivery restart/rollback tests.
- Durable provider-requested retry delays, with backoff floors, retry-budget enforcement, and restart/overflow tests.
- Test-only webhook POST adapter with TLS, input validation, redirect refusal, HTTP outcome classification, Retry-After parsing, and local mock-server/restart tests.
- In-memory loopback HTTPS certificate tests for trusted, unknown-issuer, hostname-mismatch, and expired certificates; explicit regression coverage for the synchronous DNS timeout limitation.
