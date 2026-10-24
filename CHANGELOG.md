# Changelog

All notable changes will be documented here. Versioning and compatibility follow
the design handoff; no release has been published.

## [Unreleased]

### Fixed

- Killed-owner process acceptance allows bounded interpreter startup under parallel
  test load while still requiring proof that the child acquired its lock before
  termination. Runtime ownership/deadline behavior is unchanged.

### Added
- Allow webhook-only foreground monitoring through the existing public transaction
  path, without SMTP settings or permissions. Installed acceptance covers missing
  resources, failed dispatch, signal shutdown and frozen event preservation on restart.
  Help baselines now advertise webhook-only configurations; verified successful HTTPS
  dispatch remains a separate acceptance gap.


- Installed prototype `start --foreground --config <absolute-path>` for SMTP/mixed
  configs, with one protected snapshot/owner, selected provider children, explicit
  scheduler PATH discovery, configured cadence/deadlines, sampled health, fatal state
  handling and 130/143 signal exits. Binary acceptance covers verified STARTTLS
  delivery, blocked work, busy ownership and durable no-replay restart. Webhook-only
  monitoring remains unsupported in this increment.

- Private local continuous runner now captures and reuses one same-executable
  provider selection for SMTP/webhook children. Controlled bound-SMTP recovery
  acceptance uses the versioned selected commands; startup cancellation/resource
  precedence remains covered. Installed monitoring command activation is pending.

- Installed SMTP helper acceptance now covers verified STARTTLS/AUTH, a temporary
  final DATA refusal and a fresh helper process replaying identical frozen Date,
  Message-ID and dot-stuffed bytes before relay acceptance. Tests use an ephemeral
  loopback certificate/server; no real provider or installed monitoring is exercised.

- Installed private bound SMTP child handler now shares provider fingerprint/trust
  validation, frozen-message validation, required STARTTLS, verified TLS and AUTH
  submission codecs. Binary acceptance covers malformed input, rejected implicit
  TLS and a loopback relay without STARTTLS, proving no plaintext AUTH. The locked
  scalar `base64` dependency now builds normally without version/feature changes.

- Installed private webhook child handler now shares the bounded frame codec and
  HTTPS transport, validating public UUID/payload identity and request ceilings.
  Malformed input exits silently with 2; installed monitoring and the SMTP child
  handler remain pending. Existing locked `ureq`/`httpdate` dependencies now build
  normally, with unchanged TLS features and no added dependency versions.

- Private provider helper selection now captures the current executable and builds
  fixed versioned webhook/required-STARTTLS commands without PATH/environment helper
  discovery. Exact grammar and malformed-stdin process acceptance are covered;
  installed helper handlers remain pending.

- Private foreground summary exit mapping now returns 130/143 only for cancelled
  runs with retained SIGINT/SIGTERM reasons. Startup input/resource failures retain
  exit 2 and runtime/ownership failures retain exit 1 despite late signals. Real
  signal process acceptance verifies exact exit codes, cleanup and durable state.

- Private local continuous orchestration now binds effective-UID identity, explicit
  environment references, bounded direct scheduler execution and wall-clock samples
  to public owned ticks. Absolute scheduler paths and explicit helper commands are
  required; isolated synthetic-process acceptance covers delivery and cancellation.

- Private continuous startup/shutdown orchestration now uses owned public ticks,
  OS monotonic timing and interruptible waits. Continuous summaries preserve large
  tick counts without finite-run narrowing; cancellation/invalid-budget preflight
  and post-commit shutdown/reopen acceptance are covered.

- Private owned sessions now expose public mixed-provider ticks to the continuous
  runner. Controlled acceptance verifies exclusive ownership through poll-budget
  renewal, retained resources after config removal, retry completion, cancellation
  releasing ownership and durable pending webhook/SMTP recovery.

- Private public-event runtime ticks now share cycle preflight, finite poll budgets,
  checkpoint windows, health reporting and maintenance with mixed guarded delivery.
  Controlled acceptance covers both provider completions, query failure, exhausted
  polling and cancellation after commit with durable pending work on reopen.

- Private mixed public delivery now routes frozen HTTPS webhook requests and
  permission-guarded SMTP messages through one completion-based retry worker.
  Controlled children verify partial completion, denied SMTP without credential
  lookup, restart recovery and cancellation/provider-confusion blocking.

- Private stored-public-webhook delivery adapter rechecks endpoint revision, HTTPS
  and current payload limits, then supervises an owned secret-stdin request.
  Controlled child retry/restart acceptance preserves frozen UUIDs/bytes; changed
  resources block dispatch without consuming attempts. Installed sending is pending.

- Private mixed poll composition admits complete prepared HTTPS resources and
  bounded public payloads, then atomically freezes selected webhook/SMTP work;
  rollback leaves the caller unchanged and restart preserves both provider frames.

- Private mixed-provider storage preparation now commits frozen public webhook
  frames and SMTP messages/profile bindings with one checkpoint transaction; late
  failure rolls back both providers, including schema migration. Supervised runtime dispatch remains pending.

- Private transactional public webhook UUID-v4 mapping, frozen allowlisted JSON and
  sealed endpoint-revision bindings with atomic opt-in schema-5 migration, payload
  admission, collision/entropy/rollback/retry/restart/corruption acceptance.

- Installed metadata-only historical `status --config <absolute-path>` with sampled
  queue/checkpoint output, explicit busy/missing/unsafe/schema diagnostics, cooperative
  SQL timeout and binary acceptance proving read-only, existing-only inspection.

- Installed SMTP/mixed configuration validation with protected DER trust loading,
  current profile/rendering fingerprints, exact permission checks before credentials,
  static exit-3 denial diagnostics and no-connect/no-state binary acceptance.

- Installed webhook-only `config validate --config <absolute-path>` with protected
  bounded strict JSON loading, HTTPS/auth environment checks, exact output snapshots
  and installed permission/symlink/redaction/no-state acceptance. SMTP configs return
  an explicit unsupported-operation diagnostic pending profile validation.

- Private owned continuous sessions with bounded poll-batch renewal, retained health
  and ownership, and blocked-delivery/cancellation/failure/restart process acceptance.

- Private continuous fixed-delay scheduler preserving monotonic history and bounded
  waits; first-delivered SIGINT/SIGTERM reason retention with isolated-process acceptance.
- Approved installed `--help`/`--version` CLI, exact stream/exit snapshots and binary
  acceptance for invalid arguments, non-Unicode input and resource-free execution.

### Changed

- Record maintainer adoption of the local MVP contracts (strict JSON, foreground
  CLI, HTTPS webhook, required STARTTLS SMTP, frozen events and explicit limits).
- Empty and invalid installed invocations now exit 2; help/version exit 0. Shared
  checks and release-build CI smoke checks enforce the approved CLI baseline.


### Changed

- Enforce 90% minimum Rust workspace line coverage in CI/shared tooling; refresh badges and current documentation, archive chronological implementation notes and simplify active agent guidance.

- Refresh installed activation review with completed private OS evidence and concrete signal-reason, continuous scheduling, live-status and webhook-only gaps; propose an independently reviewable help/version slice with exact grammar/output/exit snapshots.

- Consolidate foreground readiness evidence, correct completed private Rust preparation gaps and document concrete contract/installed activation decisions and the next bounded environment adapter milestone.

- Update commit-pinned CI actions to checkout 7.0.1, upload-artifact 7.0.2 and dependency-review-action 5.0.0; retain existing inputs/gates and document Node 24 runner compatibility and all nine Dependabot assessments.

- Update private prototypes to url 2.5.8, time 0.3.55, powerfmt 0.2.1, base64 0.23.1, rusqlite 0.40.2 and rcgen 0.14.10 on the existing Rust 1.88.0 pin. Migrate checked SQLite count decoding and certificate fixture APIs; retain scalar Base64 and DER/ring-only certificate generation.

- Patch private prototype dependencies to lettre 0.11.22, url 2.5.4 and time 0.3.47; pin development/CI Rust 1.88.0 to compile their fixes, remove obsolete stacker/psm constraints and reject advisory Punycode examples before webhook access. Product support/MSRV remains undecided. Coverage startup clears previous generated profiles for each measurement.

### Added

- MIT project license and Cargo SPDX metadata; development artifacts now include LICENSE instead of the pending-license placeholder.

- Private foreground lifecycle fixtures combine effective-UID/environment resources with SIGINT/SIGTERM cancellation; bounded shutdown, bridge cleanup, unchanged frozen pending work and fresh-resource replay/status recovery are verified.

- Private unified runtime process fixtures now use effective-UID and explicit environment adapters; invalid resources fail before state creation, existing-state failures preserve bytes, and valid-resource recovery retains frozen blocked work without duplicates.

- Private active-query/send process fixtures now receive SIGINT/SIGTERM, preserve committed frozen work and partial delivery, and verify bounded shutdown, malformed-config restart and recovery without duplicate events.

- Private owned foreground process acceptance delivers SIGINT/SIGTERM through the unified runtime, verifies bounded cancellation and bridge cleanup, and preserves frozen pending work/checkpoints across ownership release and replay.

- Private Linux/macOS SIGINT/SIGTERM bridge requests shared cancellation outside signal context, retains/restores prior handlers and joins its worker; isolated process acceptance covers sticky signals, exclusive binding and clean rebinding.

- Private Linux/macOS effective-UID account lookup uses fixed zeroizing scratch storage, static failures and shared observation identity validation; boundary and isolated host-composition acceptance preserve installed behavior.

- Private explicit environment lookup validates references and byte ceilings before access, preserves owned zeroizing bytes and rejects missing/non-Unicode/oversized values with static errors; isolated bounded child acceptance composes SMTP and webhook resources.

- Unified private runtime process fixtures now use the reported orchestration adapter directly; shared cancellation, protected source replacement, failed restart and durable frozen/partial-delivery recovery acceptance cover the single entry path.

- Private reported runtime orchestration combines protected startup, real interruptible scheduling and caller-owned ticks with one shared signal; pre-access budget validation and committed-query cancellation/recovery tests return bounded summaries.

- Combined private reported lifecycle acceptance replaces protected configuration during active callbacks, verifies owned-plan cancellation, reports invalid restart without state changes, then restores configuration and recovers only durable remaining work.

- Private process startup reporting tests cover missing/malformed/unsafe protected config, cancellation, invalid budgets, unsafe state and owner contention; exact bounded diagnostics and unchanged-state reacquisition are verified.

- Private bounded startup reports classify cancellation, limits, preparation and ownership failures without nested diagnostics; failure-ordering, malformed input, contention and successful reacquisition tests preserve session behavior.

- Private process-boundary shutdown reporting covers shared-signal cancellation, exhausted blocked replay and wait failure; exact bounded diagnostics, ownership release and unchanged durable work are verified.

- Private foreground shutdown reports classify cancellation, budget exhaustion and static failures with bounded counts/unknown error counts and 96-byte ASCII summaries; owned shutdown and metadata-exclusion acceptance.

- Private active-tick process acceptance cancels accounting-query and ordinary-send callbacks; owner release, frozen pending-work replay and exactly remaining partial-delivery recovery are verified with static child diagnostics.

- Private interruptible composed ticks use shared cancellation at stage/batch boundaries; query completion preserves frozen work, partial delivery records committed progress, and restart processes only remaining work.

- Private foreground child acceptance uses real monotonic/condition-variable waits for cross-process cancellation, forced termination and timed exhaustion; durable pending messages, bindings and attempts survive replay.

- Private caller-owned sticky cancellation wakes bounded condition-variable waits; owned sessions combine it with real monotonic scheduling, with timeout, shared cancellation, poison and cancelled restart/lock-release acceptance.

- Private foreground process acceptance covers cancellation while waiting, forced owner termination and wait failure after committed blocked SMTP work; bounded static diagnostics and child replay verify unchanged frozen bindings, attempts and deduplication.

- Private foreground scheduling uses monotonic completion-relative delays, finite tick/wake budgets and interruptible injected waits; delayed/blocked tick, cancellation, clock/error and owner-release/restart acceptance.

- Private foreground sessions own a protected mixed plan, guarded state and cycle health; startup/active cancellation, stop, ownership, live status and frozen pending-work restart tests, without scheduling or installed activation.

- Private status inspection samples cooperative cancellation before access, after acquisition and after reading; process acceptance verifies owner exit, cancellation, forced termination, fixed diagnostics and unchanged pending state.

- Private metadata-only status inspection opens existing guarded SQLite state read-only without creation, migration or repair; active-owner, unsafe/recovery-artifact, schema/header and unchanged pending-state acceptance.

- Private metadata-only status preparation validates protected configuration without identity, environment or trust access; ownership, unavailable-resource, validation and reopened pressure/freshness tests, without creating state or introducing commands.

- Private composed status snapshots read bounded health/checkpoint metadata in one transaction, report freshness and capacity pressure without lookup/send hooks, and preserve pending frozen work; static-error, privacy and restart tests.

- Private composed cycles apply independent retained-identity/event budgets through reconciliation, bounded restore, preflight and health; atomic overflow and owned-state boundary/restart recovery acceptance, preserving shared-limit adapters.

- Private composed owned-state acquisition checks exact storage-path compatibility and reuses guarded SQLite ownership; creation/mode/contention/unsafe-path acceptance and file-backed retry/restart recovery through owned state.

- Private protected-file mixed configuration preparation reads one bounded snapshot before validation/lookups; read-failure and replacement ownership tests, with file-backed composed retry/permission-denial/restart acceptance.

- Private per-destination SMTP cycle deadlines retain each composed timeout, reject incomplete/invalid maps before I/O and preserve frozen bindings through timeout edits and restart; controlled child acceptance verifies distinct retry/completion budgets.

- Private composed mixed runtime cycles map validated controls and exact permissions into supervised SMTP dispatch; cancellation, retry/ordinary progress, denied restart and rotated-credential recovery acceptance, with explicit rejection of differing SMTP timeouts.

- Private mixed draft composition owns protected SMTP trust, rendering/provider bindings and explicit permission tuples; four failure-ordering, credential/reference/trust rotation and durable selected-poll restart tests, without installed activation.

- Private webhook draft composition maps validated identity, controls, routes and owned injected resources into durable routed polling; five lookup-order/ownership/emit-suppress restart tests. Mixed SMTP drafts fail before lookup until bound composition is implemented.

- Private Rust draft configuration validation checks fields/types/version, provider and resource references, route and SMTP permission membership, numeric bounds/relationships and canonical seeds before lookup; five mutation/ownership tests, without installed schema adoption.

- Private bounded Rust JSON decoding with pinned test-only serde_json/serde_core, decoded duplicate-key rejection, canonical u64 tokens, static errors and owned zeroizing text; three syntax/structure/boundary tests, without installed schema adoption.

- Consolidated source-linked prototype readiness and contract review gates, distinguished installed resource/payload gaps and documented the activation sequence; refreshed README validation and CI descriptions.

- Private event-selected mixed runtime cycles reuse bounded poll/dispatch/maintenance stages; controlled helper acceptance covers cancellation, denied permission, retry/reopen and edited rules without retargeting frozen SMTP work or backfilling unmatched events.

- Private selected mixed-route dispatch acceptance covers ordinary delivery alongside SMTP retry, permission denial after reopen and rotated-credential recovery with unchanged frozen bytes and bindings.

- Private mixed event-selected SMTP preparation counts only selected pairs, rejects invalid selections and commits frozen messages/profile bindings with ordinary routes atomically; budget, injected-write rollback and reopen preservation acceptance.

- Private event-specific routed polling commits selected outbox pairs with the existing jobs/event/checkpoint transaction; injected enqueue failure and restart tests verify rollback, unmatched-event deduplication and unchanged pending destinations after route edits.

- Private bounded lifecycle route selection with owned aliases, per-event destination unions, duplicate/reference rejection and three boundary tests; no installed routing or public event contract.

- Private injected scheduler identity and initial-observation mapping with user-only preflight, owned zeroizing metadata, static failures and three policy/identity/durable emit-suppress tests; no effective-user adapter or installed activation.

- Private mapped-control runtime-cycle acceptance verifies query budgets, deterministic retry due time, persisted event/attempt recovery, stale health after poll failure and delayed retention cleanup without replay.

- Private typed operational-control mapping into accounting, retry and maintenance settings, with explicit numeric ceilings, cross-field checks, lossless jitter-seed conversion and three boundary/failure tests; no JSON parser, defaults or installed activation.

- Owned webhook resource frames in supervised HTTP/restart acceptance, preserving zeroizing input through child transfer and rejecting missing/invalid URL or authorization before child creation; pending attempts survive reopen and recover through rate-limit retry.

- Private injected webhook URL/authorization resolution with bounded owned zeroizing bytes, shared endpoint/header validation, static failures and four preflight/ownership/frame tests; no ambient lookup, network access or installed activation.

- Composed private trust/credential resolution in supervised loopback STARTTLS acceptance, including resource blocks before child creation, retry/lost-result restart preservation and rotated-password recovery with verified frozen bytes.

- Private composed-resource pending-delivery acceptance verifies permission/cancellation gates, trust/credential blocks without consuming attempts, frozen bytes and binding/checkpoint preservation across reopen, and rotated-password recovery through synthetic send callbacks.

- Private SMTP resource composition joins owned trust and injected credentials to immutable provider bindings, with profile/trust gates before lookup and five rotation/failure tests; no permission grant, delivery or installed activation.

- Private injected SMTP credential resolution with preflight reference/identity checks, bounded zeroizing values, exact identity matching and four ownership/rotation/failure tests; no ambient environment lookup or installed activation.

- Private owned SMTP DER trust-set loading through protected file descriptors, enforcing 32 roots and a shared 64 KiB read budget, validating content fingerprints and rejecting duplicates; three rotation/failure/budget tests, no installed loader.

- Three private configuration mutation acceptance tests for atomic file replacement, symlink substitution, same-size rewrites and permission changes; reopen behavior and static failures are verified without activating a loader.

- Private Unix configuration byte snapshots with descriptor-relative no-follow reads, owned 0700/0600 permissions, zeroizing bounded buffers, mutation checks and five filesystem/edge tests; no installed loader or public contracts.

- Private bounded Python draft-configuration fixture validation with strict key/integer/count checks, static diagnostics and 11 failure/edge tests included in shared CI/hooks; no installed parser or approved contracts.

- Draft configuration resource ceilings, strict integer/cross-field rules, explicit maintenance fields and a machine-readable review manifest with updated synthetic examples; no runtime or approved schema changes.

- Draft local CLI/config/event contract review package, SMTP configuration extension and synthetic mixed-provider example; no approved contracts or runtime activation.

- Mixed-route cancellation and blocked-resolution restart acceptance, verifying preserved SMTP snapshots/bindings, untouched attempts, skipped helper/maintenance work and later recovery; refreshed prototype readiness assessment.

- Private mixed-route orchestration with explicit ordinary destinations, preserved bound SMTP permissions, durable-artifact downgrade rejection, partial-progress/reopen tests and mixed owned TLS retry/lost-result acceptance.

- Private SMTP-only orchestration composing provider-aware atomic polling, pair permissions, local resource resolution and supervised bound submission, with cancellation, denial, retry and owned TLS restart/lost-result acceptance.

- Private bounded, cluster-scoped cleanup of unreferenced SMTP provider revisions, with transactional rollback, pending-message protection and completed-retention/reopen tests.

- Private supervised bound SMTP STARTTLS submission using the persisted relay, TLS name, trust roots and AUTH identity, with bounded zeroizing stdin, child revalidation and owned endpoint/restart/lost-result acceptance. Implicit TLS remains blocked.

- Private SMTP rendering/provider-pair permissions and opaque validated resolution contexts, checking actual trust content and authentication identity before injected dispatch or retry retirement; reopen, denial and secret-rotation tests. Concrete bound transport remains pending.

- Private immutable provider-aware SMTP polling with complete profile preflight, atomic rendering/provider bindings, changed-policy reopen recovery and rollback/suppression tests. Exact bound-profile dispatch remains pending.

- Private schema v4 for immutable SMTP provider revisions and cluster-coherent bindings, atomic prepared persistence, bounded loading and migration/conflict/rollback/reopen tests. Provider-aware polling and resolved dispatch remain pending.

- Private bounded SMTP provider-profile descriptors for relay/TLS/trust/auth identity and credential references, canonical stored validation, DER trust-set fingerprints and profile mismatch tests. Provider persistence and resolved transport integration remain pending.

- Private revision-aware SMTP delivery permissions checked before sends and retry retirement, bounded unreferenced revision cleanup, and owned TLS retry/lost-result reopen acceptance. Durable relay/TLS/auth profile binding remains pending.

- Private revision-bound atomic SMTP polling and v2 Message-ID selection, preserving original/legacy snapshots and suppressed history with policy-change, rollback, conflict, reopen and preflight tests. Revision-aware dispatch and provider profiles remain pending.

- Private additive schema v3 for immutable SMTP rendering revisions and snapshot bindings, bounded canonical loading, legacy preservation, transactional preparation and migration/reopen/conflict/rollback tests. Atomic poll and revision-aware dispatch integration remain pending.

- Private bounded canonical SMTP rendering descriptors and SHA-256 revision fingerprints from validated plans, with golden encoding/digest, policy/version changes, route-order stability, limits and invalid-policy tests. No durable binding yet.

- Private durable SMTP route-revision design covering canonical rendering fingerprints, atomic outbox bindings, legacy handling, explicit pending-revision permission, cleanup and the required later relay/TLS/auth profile binding. Implementation is pending.

- Owned TLS retry/lost-child-result acceptance through validated rendering ticks, checking selected Message-ID/Date headers, original snapshots despite changed route policy, completion-based retries and exact durable attempt history.

- Private per-event SMTP Message-ID selection using bounded, length-prefixed SHA-256 event/route/namespace fields and observed-time frozen Dates; shared namespaces, cancellation context, actual-header uniqueness and reopen replay tests. Reuse locked ring 0.17.14 as a direct dev dependency without package version changes.

- Private orchestration using validated SMTP rendering plans after cancellation and before clocks/storage, retaining poll budgets, frozen-message maintenance after rendering failure and owned TLS blocked-dispatch restart recovery.

- Private immutable SMTP rendering plans validating bounded route bindings, explicit disclosure and message metadata before scheduler polling, with event-dependent failure/reopen recovery and frozen-disclosure replay tests.

- Mixed-route SMTP disclosure and late rendering-failure acceptance, preserving earlier poll state and frozen snapshots, recovering through owned TLS delivery after reopen and suppressing duplicate preparation/sends.

- Private bounded SMTP lifecycle rendering with fixed state labels, explicit hidden/numeric job-ID disclosure, omitted cluster/attempt context, static rejection diagnostics and atomic preparation/reopen recovery tests.

- Owned prepared SMTP tick acceptance for temporary refusal and lost-result retries, frozen-byte replay after reopen, deduplicated polling, and partial delivery recovery after helper spawn failure.

- Private prepared SMTP orchestration sharing delivery, retention, health and cancellation handling; route preflight before side effects, frozen-message restart recovery and continued pending delivery after preparation failure.

- Private prepared SMTP cycle steps sharing durable restoration, accounting windows, finite poll budgets and health tracking, with distinct preparation/limit failures, checkpoint-preserving restart recovery and owned SMTP cycle-to-delivery acceptance.

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
