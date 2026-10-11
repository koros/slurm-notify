# Changelog

All notable changes will be documented here. Versioning and compatibility follow
the design handoff; no release has been published.

## [Unreleased]

### Changed

- Patch private prototype dependencies to lettre 0.11.22, url 2.5.4 and time 0.3.47; pin development/CI Rust 1.88.0 to compile their fixes, remove obsolete stacker/psm constraints and reject advisory Punycode examples before webhook access. Product support/MSRV remains undecided. Coverage startup clears previous generated profiles for each measurement.

### Added

- Private durable scheduler polling with fallible per-event/SMTP-route fixture preparation, explicit message-count preflight, atomic snapshot persistence and in-memory publication after commit; preparation/query/storage failure, suppression, replay and owned SMTP restart coverage.

- Private atomic poll preparation committing jobs, lifecycle ledger, mixed outbox routes, per-destination frozen SMTP messages and checkpoint together; missing/extra preparation rejection, late-write rollback, replay/legacy preservation and owned SMTP restart acceptance.

- Separate bounded private frozen SMTP child frame, exact stored envelope/DATA submission through retry/restart, lost-result replay, and missing/corrupt snapshot blocking before connection.

- Private version-2 frozen SMTP envelope/DATA storage with immutable preparation, bounded loads, version-1 upgrade, rollback/corruption checks and exact retry/reopen preservation.

- Private SMTP submission outbox policy preserving phase/acceptance evidence, durable accepted/rejected/retry/block partial progress, retry exhaustion and relay-accepted/lost-child-result replay fixtures.

- Bounded private SMTP submission child requests and canonical phase/acceptance acknowledgments, with supervised final DATA stall/drip/EOF, relay acceptance/refusal, preflight no-connect and wrong/failed-child checks.

- Private verified/authenticated SMTP submission with bounded phase replies, one validated envelope/DATA transaction, final-acknowledgment acceptance policy, write-failure fixtures, and real TLS message observation.

- Bounded private single-recipient SMTP message builder, stable identity/date, injection and ASCII-input validation, CRLF/DATA transparency tests; test-only lettre builder with documented compiler compatibility pins.

- Private authenticated-SMTP outbox policy: temporary failures retry, credential/mechanism/permanent failures block, authentication success stays pending; owned-store restart and partial-progress acceptance.

- Supervised private verified-TLS authentication child fixtures, zeroizing stdin ownership, distinct sanitized TLS/AUTH results, and bounded slow/drip-fed/disconnected AUTH acceptance without message submission.

- Separate bounded private SMTP credential frames with zeroized owned buffers and strict authentication-only acknowledgments; malformed/truncated inputs and cross-mode tokens are rejected.

- GitHub CI/coverage/toolchain badges, Linux/macOS checks and rustdoc validation, an enforced 80% Rust line-coverage gate with report artifacts, workflow linting, PR dependency review, weekly dependency updates, and gated checksummed development build artifacts.

- Bounded private AUTH PLAIN credentials and zeroized command buffers, with verified-STARTTLS/fresh-EHLO authentication fixtures and static reply/certificate failure checks; reused locked base64 as a test-only direct dependency.

- Precise private STARTTLS missing/temporary/permanent refusal diagnostics, retry-versus-block policy, strict replacement tokens, and real child/owned-storage reopen acceptance.

- Owned SMTP negotiation acceptance composing real child I/O/timeout retries, later certificate blocks, retained partial progress, ownership, and repeated-block reopen checks.

- Private SMTP negotiation dispatch policy with explicit not-submitted evidence, pending-row preservation for blocks/READY, and completion-based retry/reopen/attempt-budget bookkeeping tests.

- Strict private SMTP negotiation acknowledgments preserving static diagnostics across a shared supervisor decoder boundary, with real certificate/protocol, wrong-token, and failed-child process checks.

- Static private TLS failure categories based on typed rustls causes, with certificate regression checks and supervised stalled, drip-fed, and disconnected STARTTLS handshake fixtures.

- Separate loopback-only SMTP negotiation child with a bounded port frame and supervised stalled/drip-fed greeting and STARTTLS reply deadline checks, plus refusal and malformed-frame controls.

- Private required-STARTTLS negotiation with bounded capability inspection, missing/refused upgrade checks, verified post-upgrade EHLO, and certificate failure fixtures.

- Private implicit-TLS greeting/EHLO experiment with trusted, unknown-issuer, wrong-hostname, expired-certificate, and invalid-reply loopback fixtures; no SMTP submission or authentication.

- Source-based SMTP dependency assessment deferring lettre transport for compiler and reply-allocation constraints, with a private TLS boundary experiment planned.

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
- Private orchestration preflight for scheduler settings, unique nonempty destinations, numeric limits, window/retry policies, clock range, and retry overflow, with side-effect rejection and valid-boundary tests.
- Owned orchestration acceptance for supervised HTTP acknowledgment timeout, persisted completion-based retry, pending-send cleanup protection, restart recovery with stable ID, and post-recovery deduplication.
- Private supervised-provider classification with explicit acceptance uncertainty, static parent diagnostics, blocked local dispatch failures, preserved retry hints, and timeout recovery harness integration.
- Private fallible provider worker callbacks that stop blocked dispatch without fabricated attempts, preserve earlier commits and pending identities across restart, and retain classified timeout/provider-delay retries.
- Private fallible orchestration with blocked-dispatch maintenance stop and owned process acceptance proving partial delivery commits survive helper spawn failure and restart recovery without resends.
- Private partial delivery failure reports with committed outcome counts, static failure/dispatch categories, and unrecorded-send uncertainty, retained by orchestration and verified in blocked-helper acceptance.
- Mixed-outcome partial-progress tests matching retries, terminal rejection, and budget retirement to reopened durable state after a later commit failure; retirement rollback/recovery and sanitized error assertions.
- Private delivery cancellation before storage reads and between destinations, retaining committed progress and pending attempts, with mid-send completion and retry-deadline restart tests.
- Controlled delivery cancellation wired into private orchestration, with explicit cancelled stage/partial progress and owned supervised HTTP acceptance for mid-batch cancellation and restart recovery.
- Private orchestration capacity preflight with sanitized job/event pressure snapshots, preserved pending/deduplication history and budgets, and explicit-limit restart recovery tests.
- Private preflight restoration and checkpoint validation even with exhausted polling, preventing maintenance on malformed/regressed accounting state or inconsistent ledger identity, with pending-state/recovery tests.
- Consolidated private orchestration process fixtures and added missing-checkpoint maintenance, cancellation-before-preflight, and owned-store lock-lifetime acceptance coverage.
- Proposed private SMTP boundary and acceptance ADR covering explicit TLS/authentication, single-recipient transactions, phase-aware failures, bounded child execution, privacy, dependency gates, and restart uncertainty.
- Private synthetic SMTP phase policy requiring verified TLS and secure EHLO before submission, final DATA acknowledgment for relay acceptance, distinct auth/TLS blocks and reply classes, and sticky terminal outcomes.
- Private bounded ASCII SMTP reply reader with consistent multiline codes, strict CRLF, unread following replies, static errors, explicit byte/line limits, and malformed/truncated/I/O boundary tests.
- Loopback SMTP envelope-dialogue fixtures composing bounded reply parsing and phase policy, with multiline recipient replies and successful/temporary/permanent/truncated/malformed/delayed final acknowledgment coverage.
