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
- Private Linux/macOS transport supervisor with bounded stdin/stdout, cleared child environment, deadline and process-group cleanup, strict acknowledgment parsing, and timeout/restart tests.
- Connected the private webhook adapter to a supervised test child, with bounded frame decoding and process-boundary HTTP, TLS rejection, slow-response, and blocked-resolver acceptance checks.
- Completion-clock retry scheduling and HTTP-date evaluation, with slow-send, restart, clock-regression, and overflow coverage; removed the stale batch timestamp from private child requests.
- Explicit bounded deterministic retry jitter with provider-delay floors, persisted due times, and overflow checks; sanitized HTTP/transport diagnostic categories and permanent configuration-error classification.
- Unapproved webhook payload/configuration review drafts with synthetic JSON examples, privacy and identity semantics, explicit-value policy, compatibility proposals, and implementation-gap analysis.
- Private Linux/macOS state-directory creation with ownership/permission/symlink/hard-link checks, lifetime advisory locking, SQLite no-follow opening, and competing/killed-owner recovery tests.
- Integrated owned storage, durable mock polling, and supervised HTTP delivery in restart/retry and accepted-send rollback/replay scenarios, including lifetime competing-owner checks.
- Private UTC accounting-window planning with explicit overlap/lookback limits, catch-up gap flags, clock validation, and delayed-record restart/checkpoint tests.
- Private caller-driven finite polling cycles composing durable restoration and accounting-window planning, with cancellation, exhausted-budget, restart, query/commit failure, and catch-up gap tests.
- Read-only bounded retention planning for completed destination deliveries, preserving pending/uncertain completion records, event/job deduplication history, and checkpoints, with age-boundary and reopen tests.
- Private atomic completed-delivery cleanup with bounded candidate batches, status/age/latest-attempt revalidation, stale-plan rejection, rollback injection, and restart deduplication tests.
- Private sanitized storage health snapshots with bounded destination samples, due/pending/completed counts, cleanup eligibility, uncertain completion protection, restoration-capacity probes, and cleanup/reopen/truncation/error tests.
- Private poll-health tracking with saturating failure streaks, sanitized source categories, durable checkpoint age/staleness, composed storage reports, and recovery/restart/commit-failure/clock tests.
- Private serialized orchestration of polling, due delivery, completed-delivery cleanup, and health reporting, with stage cancellation, scheduler failure isolation, clock checks, uncertain-send stop behavior, and restart tests.
- Owned-store orchestration acceptance through the supervised HTTP child, covering cancellation after poll commit, persisted 429 retry delay, restart delivery during scheduler failure, cleanup, deduplication, and competing-owner rejection.
- Process acceptance for cancellation after two supervised HTTP deliveries, cleanup rollback on a later destination, continued health reporting, restart maintenance retry, and no notification replay.
