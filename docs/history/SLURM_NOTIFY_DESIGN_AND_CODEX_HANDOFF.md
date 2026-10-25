# Slurm Notify — Product, Architecture & Codex Implementation Specification

**Status:** Historical design draft; current status is in README.md
**Version:** 0.1 design draft  
**Date:** 2026-10-10  
**Audience:** Codex, maintainers, contributors, HPC researchers

> **Current guidance:** This document preserves the original design. Its initial
> execution instructions and open decisions are historical, not current agent
> instructions. Follow AGENTS.md and the current implementation plan. Licensing
> is now MIT and the minimum Rust coverage gate is 90%.

> **Codex handoff:** Read this document in full. First inspect the repository and produce a gap analysis and implementation plan. Do not assume a repository already exists. Do not implement every phase at once. Implement one approved milestone at a time with tests and documentation. Treat requirements labeled MUST as release gates, and flag any cluster-specific assumptions for validation.

## 1. Executive summary

Slurm Notify is a **user-space, cross-platform, scheduler-aware notification agent**, written in **Rust**, initially supporting Slurm. It monitors jobs belonging to a researcher, detects lifecycle events and optionally analyzes accessible stdout/stderr for configurable severity and error patterns, and routes normalized events to configurable notification destinations (email, Slack, Microsoft Teams, generic webhooks, and future integrations). It must not require researchers to compile code, understand Rust, modify existing batch scripts for basic lifecycle notifications, or obtain cluster administrator privileges.

The product is intended as an independently useful open-source utility, not a dissertation-only script. User experience, portability, predictable behaviour, low overhead, privacy, and safe release governance take priority over developer implementation speed. GitHub Actions should produce downloadable binaries and release artifacts.

**Non-goals for initial release:** replacing Slurm accounting; modifying Slurm server configuration; guaranteeing exactly-once delivery across external providers; interpreting arbitrary logs with perfect accuracy; automatic job cancellation; a hosted cloud service; automatic installation of persistent processes where cluster policy forbids them.

## 2. Guiding principles and constraints

1. **Five-minute first notification:** a typical researcher with a supported system, Slurm access, and a configured provider should complete installation, setup, and a test alert in approximately five minutes. This is a UX target, not a guarantee.
2. **No root:** installation in user-owned directories; never require sudo for the standard flow.
3. **No compilation:** prebuilt binaries with documented platform and libc compatibility.
4. **No batch-script edits for lifecycle alerts:** discover current user's jobs using available Slurm commands; log monitoring may require discoverable/readable output paths.
5. **Provider-neutral:** no Slack-specific assumptions in the event model or rules engine.
6. **Least privilege and privacy:** inspect only jobs/logs accessible to the user; never transmit complete logs by default.
7. **Safe and respectful to shared clusters:** bounded polling, concurrency, log reads, and process footprint; honour local HPC policies.
8. **Fail visibly, not silently:** clear status/doctor output, bounded retries, durable state, meaningful diagnostics.
9. **Cross-platform:** Linux local Slurm mode first; remote SSH mode later for Linux/macOS/Windows.
10. **Stable contracts:** CLI, configuration, events, and any exported Rust library APIs must be governed and versioned.

## 3. Personas and primary user journeys

### 3.1 Researcher on Linux HPC login node

Researcher downloads a Linux binary to `~/.local/bin`, runs `slurm-notify init`, selects a provider, configures delivery credentials, runs `slurm-notify test`, then enables monitoring using a supported policy-compliant execution method. Existing `sbatch train.sh` continues unchanged. Researcher receives job completion/failure alerts and, if enabled and accessible, early application error alerts.

### 3.2 Researcher monitoring remotely

Researcher installs the macOS/Windows/Linux binary on a workstation or permitted VPS, configures SSH to the cluster, and monitors their Slurm jobs remotely. Disconnection must be reported, with state reconciliation when connectivity returns. Remote mode requires access to relevant Slurm commands and log paths over SSH; it must not presume that SSH is always permitted or that log files are visible.

### 3.3 Maintainer/contributor

Contributor runs formatter, linter, unit/integration tests, API contract checks, security checks, and builds without cluster access. Pull requests cannot silently modify approved public contracts or publish releases.

## 4. Scope and feature priorities

| Feature | Priority | Initial acceptance criterion |
|---|---|---|
| Local Slurm job discovery | P0 | Find current user's jobs without changing sbatch scripts |
| Lifecycle events | P0 | Detect started, completed, failed, timeout, cancelled, OOM where exposed |
| CLI setup wizard | P0 | Configure at least one channel without hand-editing YAML |
| Generic webhook provider | P0 | Deliver normalized events via configured HTTP request |
| Email provider | P0 | Send via configured SMTP, secrets protected |
| Slack and Teams adapters | P0/P1 | Validate provider-specific payloads and test delivery; prefer P1 if necessary for safe MVP |
| Durable state, deduplication | P0 | Survive restart without routinely replaying alerts |
| Configurable polling and filters | P0 | Respect limits; filter current user's jobs |
| `doctor`, `status`, `test` | P0 | Diagnose Slurm/provider/config issues |
| Log monitoring | P1 | Follow accessible stdout/stderr; detect configured patterns |
| Multiline exception classification | P1 | Recognize Python tracebacks and known CUDA/NCCL patterns |
| Remote SSH mode | P1 | Work from macOS/Windows/Linux with documented limitations |
| Plugin protocol | P2 | External process-based providers with versioned JSON protocol |
| Multiple clusters | P2 | Isolated cluster credentials and state |
| Web dashboard / AI summaries | Out of initial scope | Separate design approval required |

P0 is required for the first public MVP unless explicitly rescheduled with documented rationale. P1 follows in incremental releases.

## 5. Architecture

```text
                +-------------------------------+
                | Slurm Adapter                 |
                | squeue / sacct / scontrol     |
                | optional SSH transport        |
                +---------------+---------------+
                                |
                 +--------------v--------------+
                 | Job State Reconciler         |
                 | snapshots, transitions, IDs  |
                 +--------------+--------------+
                                |
   +-------------------+        |        +----------------------+
   | Log Reader        +--------+        | Health / Diagnostics   |
   | tail, parse,      |                 | permissions, lag,      |
   | classify (P1)     |                 | delivery failures      |
   +-------------------+                 +----------------------+
                                |
                 +--------------v--------------+
                 | Normalized Event Bus         |
                 | job identity, event ID,      |
                 | severity, timestamps, source |
                 +--------------+--------------+
                                |
                 +--------------v--------------+
                 | Rules / Routing Engine       |
                 | filters, thresholds, cooldown|
                 +--------------+--------------+
                                |
                 +--------------v--------------+
                 | Durable Outbox (SQLite)      |
                 | retries, delivery state      |
                 +--------------+--------------+
                                |
                 +--------------v--------------+
                 | Notification Provider Trait  |
                 +-----+------+-------+---------+
                       |      |       |
                     SMTP   Webhook  Slack/Teams
```

**Separation of concerns:** scheduler adapter returns normalized job records; state reconciler produces lifecycle events; log reader emits application events; routing decides recipients; delivery providers only transform/send; storage persists reconciliation and delivery checkpoints. No provider should query Slurm or read files itself.

### 5.1 Proposed Rust workspace

```text
slurm-notify/
  AGENTS.md
  README.md
  CHANGELOG.md
  CONTRIBUTING.md
  SECURITY.md
  CODE_OF_CONDUCT.md
  LICENSE
  Cargo.toml
  Cargo.lock
  rust-toolchain.toml
  .github/
    workflows/ci.yml
    workflows/compatibility.yml
    workflows/security.yml
    workflows/release.yml
    CODEOWNERS
    dependabot.yml
    pull_request_template.md
  crates/
    notify-core/           # events, rules, contracts; minimal public API
    notify-slurm/          # scheduler and transport adapters
    notify-providers/      # webhook, SMTP, Slack, Teams
    notify-storage/        # SQLite, migrations, outbox
    notify-cli/            # CLI and wizard, executable
  contracts/
    cli/
    config/
    events/
    public-api/
  docs/
    architecture/
    adr/
    installation/
    operations/
    providers/
  tests/
    fixtures/
    integration/
    e2e/
```

Do not create needless crates merely to match the diagram. Start with a small workspace and split when justified. Keep internals private; expose only intentionally supported APIs.

### 5.2 Suggested libraries

- `tokio` async runtime; `clap` CLI; `serde` serialization; a maintained YAML parser with assessed maintenance/security status; `tracing` diagnostics; `reqwest` HTTP; `lettre` SMTP; `rusqlite` or `sqlx` SQLite; `regex` patterns; `thiserror` and `anyhow` errors; `secrecy` or equivalent secret handling.
- `wiremock` or equivalent for HTTP integration tests; `assert_cmd` for CLI tests; `tempfile` for filesystem fixtures; `cargo-nextest`, `cargo-llvm-cov` for CI where appropriate.
- Assess transitive dependencies, MSRV, license, platform support, and binary size before adoption.

## 6. Slurm integration details

### 6.1 Commands and permissions

Use available `squeue`, `sacct`, and optionally `scontrol` as subprocesses with argument arrays (never shell-interpolate untrusted data). Prefer parseable/parsable output formats and explicit fields. `squeue` is typically for active jobs; `sacct` provides historical/accounting records if enabled. Do not assume every cluster has complete accounting, immediate accounting updates, or identical Slurm versions. Discover available features during `doctor` and degrade gracefully.

Use stable identity comprising **cluster identifier + job ID + array task ID where applicable + submission/start attempt context**. Track job steps separately where needed. Handle requeue, restart, job arrays, heterogeneous jobs, cancellation, preemption, OOM, timeout, node failure, and transient unknown states. Treat Slurm's state as authoritative for scheduler state, not application logs. `ExitCode` and `DerivedExitCode` are different; record their meanings and avoid claiming they are always equivalent.

### 6.2 Polling and reconciliation

- Configurable intervals (default proposal: 30 seconds for active jobs, slower for history; validate cluster guidance).
- Bounded concurrent subprocesses; command timeouts; exponential backoff with jitter on failures.
- Track last successful query and reconcile state transitions after downtime.
- Avoid missed short jobs: query a bounded historical window, not only `squeue`.
- Persist high-water marks/checkpoints; clock skew and late accounting updates must be considered.
- Do not hammer `sacct` or poll individual jobs unnecessarily; batch queries where practical.
- Define alert semantics for jobs discovered after completion and for pre-existing jobs when agent first starts.

### 6.3 Log path discovery and reading (P1)

Use Slurm job metadata where available to discover stdout/stderr paths. Respect configured filename tokens, output redirection, working directories, permissions, and remote filesystem limitations. Some jobs combine stdout/stderr; avoid double counting. Files may not exist yet, may buffer writes, rotate, truncate, or be deleted. Store file identity and offset; on restart detect rotation/truncation. Use bounded reads and buffers. No guarantee of real-time alerts when application output is buffered. Python `-u` may improve timeliness but is optional and must not be imposed on existing jobs.

Classify by configurable severity patterns, not simply by stdout versus stderr. Recognize multiline tracebacks and known patterns (`CUDA out of memory`, NCCL errors, etc.) with bounded context. An `ERROR` line does not necessarily mean the job failed; a job may fail without an `ERROR` line. Store separate scheduler status and application severity.

## 7. Normalized event contract

All event producers emit an internal, versioned event model. Suggested wire representation:

```json
{
  "schema_version": "1.0",
  "event_id": "deterministic-or-persisted-unique-id",
  "occurred_at": "2026-10-10T13:00:00Z",
  "observed_at": "2026-10-10T13:00:12Z",
  "source": "slurm",
  "cluster_id": "lovelace",
  "job": {
    "id": "12345",
    "name": "llm-training",
    "array_task_id": null,
    "state": "FAILED",
    "exit_code": "1:0"
  },
  "event_type": "JOB_FAILED",
  "severity": "ERROR",
  "summary": "Job exited unsuccessfully",
  "details": {"error_class": "UNKNOWN"}
}
```

`event_id` must be stable for duplicate detection. `occurred_at` may be unknown for some log-derived events; distinguish observed time from occurrence time. Severity (`DEBUG`, `INFO`, `WARN`, `ERROR`, `CRITICAL`) is a notification classification, not Slurm's native severity. Document mapping and allow overrides. Keep payload minimal and redact sensitive data. JSON event schema is a public contract once exposed to external webhooks/plugins.

## 8. Rules and provider abstraction

### 8.1 Example user configuration (illustrative, not yet final schema)

```yaml
schema_version: 1
monitor:
  mode: local
  poll_seconds: 30
  user_jobs_only: true
  include_existing_jobs: true
logs:
  enabled: false
  min_severity: ERROR
  patterns:
    - name: cuda_oom
      regex: 'CUDA out of memory'
      severity: ERROR
providers:
  primary:
    type: webhook
    url_env: SLURM_NOTIFY_WEBHOOK_URL
  research_email:
    type: smtp
    host: smtp.example.org
    port: 587
    username_env: SMTP_USER
    password_env: SMTP_PASSWORD
    recipients: [researcher@example.org]
routes:
  - events: [JOB_FAILED, JOB_TIMEOUT, JOB_OUT_OF_MEMORY]
    send_to: [primary, research_email]
  - events: [JOB_COMPLETED]
    send_to: [primary]
delivery:
  max_attempts: 5
  initial_backoff_seconds: 5
  cooldown_seconds: 300
```

Validate the final schema with a published machine-readable schema and representative fixtures. Never store secrets in example files or output. `url_env`/`password_env` are names of environment variables, not actual secrets. Offer interactive setup; manual YAML is optional.

### 8.2 Provider contract

A provider receives a normalized event and provider configuration, formats a platform-specific message, and returns a typed outcome. Separate formatting from transport where possible. Support provider-specific authentication, HTTP payload schemas, retries, `Retry-After`, error classification, and timeouts. A generic webhook must support configurable headers and payload templates **without exposing secrets in diagnostics**. Teams integration may use a different workflow/payload mechanism than Slack; do not assume interchangeable webhook payloads.

Provider failures are isolated. Routing can fan out to multiple destinations. Persist one delivery record per event + destination; provide a clear policy for duplicate deliveries and failure handling. External service acknowledgement does not guarantee human receipt. Prefer at-least-once retry semantics with best-effort deduplication; document limitations. Provider extension v1 can use built-in adapters and generic webhook; future plugins may be separate processes using a versioned JSON protocol rather than unsafe dynamic libraries.

## 9. CLI and onboarding UX

Proposed commands:

```text
slurm-notify --version
slurm-notify init
slurm-notify doctor
slurm-notify test [provider-name]
slurm-notify start [--foreground]
slurm-notify status
slurm-notify stop
slurm-notify logs
slurm-notify config validate
slurm-notify providers list
```

CLI commands are proposals; freeze and snapshot the public CLI contract before the first stable release. `init` should auto-detect supported Slurm commands, prompt for local vs remote mode (when available), show notification channels, collect safe configuration, validate credentials, send an opt-in test, and explain how to keep monitoring running. Provide clear errors and noninteractive options for scripts. Support accessible monochrome/no-TTY output.

**Important:** `start` must not imply a durable daemon unless it actually sets up a permitted persistent service. Offer foreground operation and documented user-systemd/launchd/Windows Task Scheduler or service integrations only where supported. Respect HPC site rules: some login nodes prohibit background agents; remote monitoring is the preferred alternative in those environments.

## 10. Persistence and operational resilience

SQLite is the default user-local store. Tables/concepts: monitored jobs, job snapshots, event ledger, delivery outbox, delivery attempts, log offsets, schema migrations, agent heartbeat/health. Database permissions must protect user data. Use transactions to atomically record events and enqueue deliveries; avoid sending an event before it is durably recorded. Reconcile after crashes, process restarts, clock shifts, network outages, and Slurm accounting lag. Use retention limits and cleanup policies. Make database migrations versioned and tested for forward upgrade; document rollback limitations when schema changes are irreversible.

Observability: structured local logs; status includes last Slurm poll, active job count, queued alerts, last delivery success/failure, provider health, and configuration errors. Never log credentials, full HTTP auth headers, or unredacted sensitive output.

## 11. Security and privacy requirements

- User-owned installation and state; no privilege escalation.
- Use secure TLS defaults; SSH host-key verification; do not disable verification to 'make it work'.
- Subprocess commands use safe argument passing, timeouts, and controlled environment.
- Store credentials outside versioned YAML; prefer environment variables or OS credential storage where supported. Restrict local file permissions.
- Default notifications to job metadata and short redacted error excerpts; full log forwarding requires explicit opt-in and clear warnings.
- Avoid accidental exfiltration of credentials, personal data, proprietary datasets, and environment variables from logs.
- Limit log read sizes, regex work, retries, concurrency, and queue growth; mitigate malicious/unbounded input.
- Generic webhook destinations can exfiltrate data; display destination clearly and require user configuration.
- Security policy, vulnerability reporting, dependency audits, and threat-model ADR before stable release.
- No telemetry or analytics collection by default.

## 12. Testing strategy and acceptance matrix

### 12.1 Unit tests (no Slurm required)

Scheduler output parsing; state transition/requeue semantics; event identity; severity matching; routing precedence; schema validation; redaction; deduplication; retry policy; provider payload formatting; log offsets and rotation; SQLite migrations.

### 12.2 Integration tests (mock dependencies)

Mock `squeue`/`sacct` executable fixtures or injected scheduler interface; fake log files; local HTTP mock server; fake SMTP; temporary SQLite DB; simulated network failures; clock control. Test: 200/202 success, 429 `Retry-After`, 401/403 terminal auth failure, 5xx transient retry, timeouts, webhook payload correctness, provider isolation, process restart and recovery, malformed output, missing commands, accounting lag, file permissions, no duplicates after restart.

### 12.3 End-to-end tests

Optional local container/VM Slurm cluster with explicit setup; no reliance on Docker alone guaranteeing a functioning Slurm control plane. Run real CPU-only jobs in a permitted environment: successful exit 0; exit 1; log `ERROR` but exit 0; timeout; cancel; arrays; requeue (where supported); log buffering. Final Lovelace smoke test only with authorized allocations and compliant background execution. Do not require GPUs for ordinary tests.

### 12.4 Required release gates

- `cargo fmt --check`, strict `clippy`, full unit/integration suite.
- CLI contract snapshot comparison and approval gate.
- Config and event JSON schema compatibility fixtures.
- Rust exported API compatibility and addition review where public library APIs exist.
- Dependency license/advisory checks and secret scanning.
- Build/install smoke test on every supported target or clearly mark untested targets.
- Packaging, checksum verification, `--version`, `doctor`, test notification smoke checks.
- Changelog and release notes validated against merged changes.

## 13. API governance and compatibility policy

**Public surfaces:** (1) CLI commands, flags, exit codes, output modes; (2) configuration schema, defaults and semantics; (3) emitted event/webhook schemas; (4) external plugin protocol, once published; (5) deliberately exported Rust library APIs, if any; (6) persistent state migration guarantees and documented paths.

- Use Semantic Versioning. Before 1.0, explicitly document compatibility expectations; do not treat 0.x as permission for undocumented breakage.
- Use `cargo-semver-checks` for Rust API breaking changes and `cargo-public-api` snapshots for **newly introduced** public items as well as removals/changes. Neither alone governs CLI/config/event semantics.
- Store approved CLI/config/event contract snapshots under `contracts/`; compare PR changes to the last approved baseline and report additions, removals, default changes, and type changes.
- Any intentional public contract change requires explicit PR label/approval, rationale, migration plan, documentation, and appropriate version bump.
- Protect contract files via CODEOWNERS and branch protection; never let CI auto-update snapshots to make a change pass.
- Require deprecation periods for removals when feasible. Add fixtures for previous supported config versions.
- Keep implementation-only modules private by default to minimize accidental Rust API surface.

## 14. GitHub repository governance

- Protected default branch, required status checks, pull requests, at least one human approval for contract/security/release changes; no direct release from unreviewed AI commits.
- `AGENTS.md` gives Codex immutable engineering expectations: plan first, smallest safe increment, tests with fixes, no unapproved API changes, no new dependencies without explanation, no silent default changes, no bypass of CI, no release publication without explicit approval.
- `CONTRIBUTING.md` with local build, tests, fixture strategy, style, commit policy, PR checklist.
- `SECURITY.md` with private reporting path and supported versions; `CODE_OF_CONDUCT.md`; chosen license (consider MIT OR Apache-2.0 after reviewing dependency licenses).
- ADRs for local vs remote mode, plugin strategy, event schema, state persistence, packaging and compatibility guarantees.
- Dependabot/Renovate for dependencies; security updates should be tested and reviewed.

## 15. CI/CD, packaging, releases and changelog

### 15.1 CI on every PR

Format, lint, tests, contract checks, dependency/security checks, documentation checks, build and smoke tests. Use least-privilege `GITHUB_TOKEN` permissions, pinned third-party Actions by commit SHA where feasible, and no untrusted PR code with release secrets.

### 15.2 Release process

Human-approved tag/release triggers GitHub Actions to build target binaries, package archives/installers, run smoke tests, generate SHA-256 checksums, produce SBOM/provenance where practical, and publish draft release artifacts for review before public release. Release notes should summarize features, fixes, breaking changes, migrations, known issues, compatibility, and upgrade instructions. Maintain `CHANGELOG.md` in Keep a Changelog style; optionally use conventional commits to draft entries but require review. Retain historical release binaries and support documented rollback where safe.

### 15.3 Target matrix

| Target | Mode | Initial priority |
|---|---|---|
| Linux x86_64 | local Slurm | P0 |
| Linux ARM64 | local Slurm (if supported) / remote | P1 |
| macOS Apple Silicon / Intel | remote SSH | P1 |
| Windows x86_64 | remote SSH | P1 |
| Debian/RPM packages | local | P2 |
| Homebrew/WinGet | remote | P2 |

Build on compatible Linux targets; newer glibc-linked binaries may not run on older HPC distributions. Consider musl where technically suitable and tested; do not promise universal static portability. Validate CPU architecture and OS before install. Prefer an inspectable install script that verifies release checksums/signatures, writes to `~/.local/bin`, and never requires sudo. Consider `cargo-dist`/`dist` after checking current support and maintenance; do not rely on a tool name without verifying its capabilities.

## 16. Documentation deliverables

README quick start; installation by platform; local/remote mode; provider-specific setup (SMTP, Slack, Teams, generic HTTP); configuration reference; event/severity semantics; troubleshooting; `doctor` examples; privacy/security model; cluster-policy etiquette; compatibility matrix; upgrade/rollback guide; contributor guide; architecture and ADRs; changelog and release notes. Every feature PR updates docs and tests.

## 17. Implementation milestones for Codex

### Phase 0 — repository foundation and plan

**Deliverables:** repository assessment or initialization, design ADRs, Cargo workspace, `AGENTS.md`, license decision placeholder, CI basics, test harness, documentation skeleton, contract governance approach. **Acceptance:** reproducible local build/tests; CI passes; no accidental public API exposure. **Do not yet claim working notifications.**

### Phase 1 — job lifecycle MVP

Implement CLI (`init`, `doctor`, `test`, `start`, `status` minimal), local Slurm adapter, mock scheduler, job reconciliation, normalized events, config validation, SQLite ledger/outbox, generic webhook provider, and mock-based tests. Add email and additional providers incrementally. **Acceptance:** simulated and permitted real CPU jobs generate correct one-time lifecycle alerts and recover from restart; no Slurm server changes.

### Phase 2 — production notification providers and UX

SMTP, Slack, Teams, provider tests, wizard, secure credential handling, template/redaction policy, retry/rate limit/health diagnostics. **Acceptance:** nontechnical user completes onboarding and test delivery without editing source or compiling.

### Phase 3 — live log analysis

Log discovery, safe tailing, rotation/restart handling, multiline exceptions, severity patterns, deduplication, bounded excerpts, privacy tests. **Acceptance:** detect log error before exit when output is flushed; correctly distinguish log severity from job state.

### Phase 4 — remote monitoring and multi-platform

SSH transport, host-key verification, connectivity/reconciliation, macOS/Windows packages, remote smoke tests. **Acceptance:** remote user can monitor supported cluster without local Slurm installation; failures are diagnosed clearly.

### Phase 5 — stable open-source release

Contract freeze, compatibility tests, release packaging/signing/provenance, changelog/release notes, install scripts, migration/rollback tests, user docs, threat model, optional third-party provider protocol. **Acceptance:** independently verifiable public release with no manual compilation.

## 18. Explicit open decisions — do not silently guess

1. Product name and executable naming/trademark availability (`slurm-notify` is a working name).
2. First stable release scope: generic webhook + SMTP, or Slack/Teams as P0.
3. Cluster policy on background processes and allowed polling intervals (especially Lovelace).
4. Remote mode auth strategy (OpenSSH subprocess vs library) and Windows compatibility.
5. Support baseline: minimum Rust version, minimum Slurm versions, Linux libc baseline.
6. Default event triggers, initial scan behaviour, historical catch-up window.
7. Log retention, redaction rules, and whether any excerpts may leave the cluster by default.
8. Licensing and contribution governance.
9. Whether the project exposes any supported Rust library API at all.
10. Exact supported configuration and event schema compatibility guarantees.

When an open decision blocks an implementation phase, Codex should present 2–3 options, trade-offs, and a recommended default, then request approval.

## 19. Initial Codex prompt (paste into desktop ChatGPT/Codex)

> You are the lead engineer for Slurm Notify. Read `SLURM_NOTIFY_DESIGN_AND_CODEX_HANDOFF.md` completely. The project is a Rust, user-space, provider-agnostic notification utility for Slurm researchers, with no administrator requirement and a five-minute onboarding UX goal. Start with **Phase 0 only**. First inspect the current repository (if any), identify conflicts or gaps relative to the design, and propose a concrete implementation plan including architecture decisions, Rust crates, CI gates, contract-governance strategy, test plan, and acceptance criteria. Do not code until the plan is approved. Avoid introducing unapproved public APIs, flags, schema fields, dependencies, or release automation. Preserve compatibility and testability. Explicitly flag cluster-specific assumptions. After approval, implement the smallest milestone, run the tests, document results, and summarize remaining risks.

## 20. Definition of done for every phase

- Scope and acceptance criteria agreed before code generation.
- Tests written for normal, failure, restart, and edge cases relevant to the change.
- Formatter, linter, tests, compatibility/security checks pass.
- No unapproved public contract changes or secrets.
- Documentation and changelog updated appropriately.
- User experience verified from a fresh install/test environment when applicable.
- Known limitations, migration impact, and next milestone recorded.

---

**Design note:** This is a handoff specification, not a claim that any implementation, CI workflow, installer, provider API, or Lovelace configuration has already been tested. Validate those in the relevant milestone.
