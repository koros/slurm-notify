# Slurm Notify

[![CI](https://github.com/koros/slurm-notify/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/koros/slurm-notify/actions/workflows/ci.yml)
[![Rust coverage gate ≥80%](https://img.shields.io/badge/Rust_coverage_gate-%E2%89%A580%25-blue)](docs/operations/ci-and-coverage.md)
[![Development Rust 1.88.0](https://img.shields.io/badge/development_Rust-1.88.0-orange?logo=rust)](rust-toolchain.toml)

Slurm Notify is a planned user-space notification agent for researchers running
Slurm jobs. Written in Rust, it will discover the current user's jobs, detect
lifecycle events, and route notifications to configurable destinations without
requiring administrator privileges or changes to existing batch scripts.

**Status: private, test-driven prototype.** Synthetic scheduler discovery and
reconciliation, durable SQLite state, supervised webhook/SMTP transports, and
caller-driven orchestration are implemented behind test-only interfaces. Tests
cover restart, retry, cancellation and bound SMTP profile protection. The executable
still reports unavailable monitoring; no usable monitoring agent or release
binaries exist yet. See the [product readiness assessment](docs/architecture/product-readiness.md)
for remaining work and review gates.
The name `slurm-notify` is a working name.

## Design and scope

The [design and Codex handoff](SLURM_NOTIFY_DESIGN_AND_CODEX_HANDOFF.md) is the
source of truth for requirements, architecture, release gates, and open decisions.
Its example commands, configuration, and event schemas are proposals, not
supported interfaces.

The first public MVP targets local Slurm monitoring on Linux x86_64, with job
lifecycle alerts, generic webhooks, SMTP, durable state, configurable polling,
and setup and diagnostic commands. Slack and Teams will be added incrementally,
with their release priority still to be decided. Log analysis and remote SSH
monitoring on Linux, macOS, and Windows are planned for later milestones.

Key design goals:

- Install prebuilt binaries in user-owned directories without root or compilation.
- Discover jobs without requiring batch-script edits for lifecycle notifications.
- Keep scheduler discovery, event reconciliation, routing, storage, and delivery separate.
- Persist events and delivery attempts, with bounded retries and best-effort deduplication.
- Protect credentials and sensitive output; never forward complete logs by default.
- Bound polling and resource usage and respect each cluster's execution policies.

## Folder structure

```text
.
├── README.md
├── SLURM_NOTIFY_DESIGN_AND_CODEX_HANDOFF.md
├── .github/
│   └── workflows/          # Future CI, compatibility, security, and release workflows
├── crates/                 # One executable crate; other boundaries reserved
│   ├── notify-core/        # Events, rules, and internal contracts
│   ├── notify-slurm/       # Scheduler and transport adapters
│   ├── notify-providers/   # Webhook, SMTP, and later Slack/Teams adapters
│   ├── notify-storage/     # SQLite persistence, migrations, and delivery outbox
│   └── notify-cli/         # Development executable; CLI and wizard planned
├── contracts/
│   ├── cli/                # Approved command, flag, output, and exit-code baselines
│   ├── config/             # Configuration schemas and compatibility fixtures
│   ├── events/             # Event schemas and compatibility fixtures
│   └── public-api/         # Deliberately supported Rust API baselines, if any
├── docs/
│   ├── architecture/       # Component responsibilities and data flow
│   ├── adr/                # Architecture decision records
│   ├── installation/       # Platform installation and upgrade instructions
│   ├── operations/         # Monitoring, diagnostics, privacy, and cluster etiquette
│   └── providers/          # Provider setup and delivery semantics
└── tests/
    ├── fixtures/           # Scheduler output, configuration, and event samples
    ├── integration/        # Mock scheduler, HTTP, SMTP, and storage tests
    └── e2e/                # Optional tests against a permitted Slurm environment
```

Empty directories contain `.gitkeep` placeholders so they can be tracked when
Git is initialized. Only `notify-cli` is a Cargo package. The other crate directories reserve the
proposed layout and will become crates only when justified.
No public Rust API is established by this scaffold.

## Getting started

There is no usable monitoring agent yet. Developers can build the foundation
with the pinned Rust toolchain:

```sh
cargo build --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s tests/integration/storage -p 'test_*.py' -v
```

The executable prints its development status and exits with code 1. The latest
implementation milestone passed 337 Rust tests plus one intentionally ignored
subprocess helper, custom process acceptance, eight Python/SQLite tests, six hook
tests and 11 draft-configuration tests. Measured Rust line coverage was 97.52%; the
CI gate requires at least 80%. These are local results, not a hosted CI status claim.

Private tests cover scheduler discovery, durable lifecycle/outbox state, supervised
webhook/SMTP delivery, protected resource snapshots, injected resource resolution,
policy mapping and selected mixed-route runtime cycles. Installed CLI commands,
configuration parsing, public webhook payload persistence and a foreground loop
remain pending. See the [product readiness assessment](docs/architecture/product-readiness.md)
for evidence, limits and the proposed activation sequence, and
[contributor guidance](CONTRIBUTING.md) for development instructions.

Install local pre-commit/pre-push checks with `sh scripts/install-hooks.sh`.
Run the shared verification suite with `sh scripts/check.sh`. See
[contributor guidance](CONTRIBUTING.md#local-git-hooks) for how staged checks work.

## Implementation milestones

1. **Repository foundation:** approve the Phase 0 plan, record architecture
   decisions, establish the Cargo workspace, contributor guidance, CI basics,
   test harness, and contract governance.
2. **Job lifecycle MVP:** implement local discovery, reconciliation, configuration,
   SQLite ledger/outbox, generic webhooks, and a minimal CLI with mock-based tests.
3. **Providers and onboarding:** add SMTP, Slack, Teams, secure setup, redaction,
   retries, and delivery diagnostics.
4. **Log analysis:** add bounded log reading, severity patterns, multiline
   exceptions, rotation handling, and privacy tests.
5. **Remote monitoring:** add SSH transport, connectivity recovery, and validated
   macOS/Windows support.
6. **Stable release:** freeze contracts, validate packaging and compatibility,
   complete security and operational documentation, and obtain release approval.

The repository, workspace, hooks, CI workflow and private engine are implemented.
Public lifecycle/provider milestones remain incomplete until installed runtime,
onboarding and required acceptance are verified. Governance, support and release
decisions remain open. Historical increments are recorded in the implementation
plan; the product readiness assessment describes current status.

## Development and validation

Ordinary development tests should run without a Slurm cluster using scheduler
fixtures, mock providers, temporary databases, and controlled failure scenarios.
The current workflow runs formatting, strict linting, unit/integration tests,
tracked-file checks, documentation checks, coverage and workflow validation, with
PR dependency review and a development placeholder build artifact. Approved
contract compatibility, supported-product install tests and release gates remain
pending. Real-cluster tests require authorized resources and
compliance with local policy.

CLI, configuration, events, intentional Rust APIs, and persistence guarantees
must be governed explicitly. Contract baselines must be reviewed rather than
automatically rewritten to make checks pass.

## Draft interface proposals

The [webhook payload proposal](docs/proposals/webhook-payload-v1.md) and
[local webhook configuration proposal](docs/proposals/local-webhook-config-v1.md)
contain synthetic examples and concrete review decisions. They are unapproved
proposals; the executable does not read that configuration or emit that payload.

## Open decisions

The handoff records unresolved decisions, including licensing, minimum Rust and
Slurm versions, Linux libc compatibility, provider release scope, default event
and catch-up behaviour, log privacy rules, remote authentication, and whether to
expose a supported Rust library API. Cluster polling intervals and background
execution permissions must be validated locally; the proposed 30-second polling
interval is not an approved operational default.

No license has been selected or granted; see [license decision placeholder](LICENSE.pending.md).
Rust 1.88.0 is pinned for foundation development only, not as a product support
baseline. Contract checks, security workflows, release automation, maintainer
ownership, and branch protection remain future work.

The private accounting-window experiment now uses explicit UTC overlap and
lookback limits, with delayed-record restart tests and flags for clipped overlap
or downtime beyond the bound. It does not guarantee complete historical recovery;
see the [implementation plan](docs/architecture/implementation-plan.md).

Private polling-cycle tests now compose restoration, window planning, and durable
polling with a finite attempt budget and cancellation between cycles. The caller
still supplies cadence and limits; no background monitoring loop is active.

Private retention tests now plan bounded cleanup candidates for old completed
outbox destinations. Planning deletes nothing; private transactional application now removes eligible
completed outbox destinations and their attempt history. Event/job history and pending or
uncertain deliveries remain protected. Ledger growth still needs a retention
horizon and tombstone design before terminal snapshots can be safely removed.

Private storage health tests report sampled queue counts, cleanup eligibility,
uncertain completion records, and restoration capacity pressure. Truncation is
explicit; reports contain no job, user, destination, or event identifiers. These
reports are not yet exposed through a `status` or `doctor` command.

Private poll-health tests track consecutive failed cycles and stale durable query
checkpoints alongside storage health. Failure streaks are session-local; reports
restore checkpoint age after restart. No public status command is active yet.

Private caller-driven orchestration tests now compose polling, due delivery,
cleanup, and health reporting. Scheduler query failure can leave committed
notifications deliverable; uncertain delivery commits stop later stages.
Cancellation is checked between stages. No operational monitoring loop is active.

The process acceptance harness now exercises composed orchestration with owned
storage and supervised loopback HTTP delivery, including cancellation, persisted
rate-limit retries across restart, scheduler failure isolation, and cleanup.
Payloads and scheduler records remain synthetic private fixtures.

The process harness also verifies cancellation after a completed delivery batch
and cleanup failure isolation: deliveries stay committed, cleanup rolls back,
health reporting continues, and maintenance succeeds after restart without resends.

Private orchestration now validates settings and policies before polling or
mutating storage. Invalid preflight leaves the poll budget and failure streak
unchanged; stage reports distinguish preflight rejection from a failed poll.

Owned process acceptance now covers a supervised HTTP acknowledgment timeout:
the pending retry survives maintenance and restart, then succeeds with the same
internal ID. Timeout acceptance remains uncertain, so retries can duplicate an
external notification when a provider does not honor idempotency keys.

Private supervised-provider classification now distinguishes acknowledged outcomes,
uncertain post-spawn failures, and local dispatch failures that require intervention.
The timeout recovery acceptance test uses this policy; blocked dispatch is now supported by a private fallible delivery worker.
Private orchestration now supports fallible callbacks and stops maintenance after
blocked dispatch; owned process tests verify recovery after helper spawn failure.

Private orchestration retains committed batch counts when a later delivery fails,
with sanitized failure categories and an explicit flag for unrecorded provider
outcomes. Failed items are excluded from committed counts.

Partial batch tests now compare mixed retry, rejection, and budget-retirement
counts with reopened database state after a later write failure. Uncommitted
outcomes remain excluded, and failed retirement can recover without sending.

The private controlled delivery worker supports cancellation before a batch and
between destinations. An active provider call completes its durable bookkeeping
before cancellation stops later sends. Orchestration now passes the cancellation
signal through, with owned supervised HTTP restart acceptance coverage.

Private orchestration now checks stored job/event capacity before polling or
sending. Over-capacity state produces a sanitized pressure report and preserves
history and pending deliveries; recovery requires an explicit caller limit change.

Exhausted polling now retains ledger/checkpoint preflight: invalid accounting times
or inconsistent restored state stop delivery and cleanup. A corrected checkpoint
allows maintenance to resume without another scheduler query.

Shared orchestration acceptance fixtures now cover missing-checkpoint maintenance
and cancellation before preflight, including retained ownership until store close.
A missing checkpoint leaves poll age unknown and stale in private health reports.

The [private SMTP boundary proposal](docs/adr/0006-private-smtp-boundary.md)
records transport, TLS/authentication, privacy, and acceptance requirements before
dependency selection. SMTP implementation and public email contracts remain pending.

A private synthetic SMTP phase classifier now tests TLS/authentication ordering,
final DATA acknowledgment, reply outcomes, and uncertainty. It performs no SMTP
network I/O and is not yet connected to a client or delivery worker.

The private SMTP reply experiment now bounds line bytes, total reply bytes, and
line count, with multiline and malformed/truncated fixtures. It discards server
text and performs no SMTP connection or TLS validation.

Loopback SMTP envelope fixtures now compose parsing and phase policy for final
acknowledgment success, rejection, and uncertainty. TLS evidence is synthetic;
these unauthenticated fixtures do not implement secure SMTP submission.

The [SMTP dependency assessment](docs/adr/0007-smtp-dependency-assessment.md)
defers lettre’s transport: inspected versions conflict with the compiler pin or
check reply lengths after line allocation. The next private experiment verifies
implicit TLS using the existing test dependency and bounded reply reader.

Private implicit-TLS fixtures now verify trusted greeting/multiline EHLO and
certificate rejection before SMTP commands, with invalid greeting/EHLO checks.
They use in-memory certificates and bounded socket waits; AUTH, submission,
STARTTLS, and an absolute supervised SMTP deadline remain unimplemented.

Required-STARTTLS fixtures now reject missing/misleading advertisements and refused
upgrades, verify certificates before a fresh EHLO, and stop on failed secure EHLO.
Only greeting and negotiation commands exist; SMTP AUTH and submission remain pending.

The private process harness now supervises SMTP negotiation separately from HTTP.
Stalled and drip-fed greeting/upgrade replies stop at the parent deadline despite
longer socket timeouts. This loopback-only probe has no credentials, message path,
or delivery acknowledgment; it is not an installed SMTP helper.

Private TLS errors now distinguish certificate, local TLS configuration, handshake
I/O, and other TLS protocol failures without retaining raw diagnostics. Supervised
STARTTLS fixtures also bound stalled/drip-fed handshakes and check disconnects after
ClientHello. Worker outcome mapping and authenticated submission remain pending.

Private SMTP negotiation diagnostics now cross the supervised boundary through
strict, bounded acknowledgments. Certificate, configuration, I/O, protocol, reply,
and upgrade categories survive; READY means negotiation only. No token means relay
acceptance, and worker retry/block bookkeeping remains pending.

Private negotiation policy now keeps certificate/input/configuration/upgrade blocks
and negotiation-only READY pending without fabricated attempts. Transient errors
use existing completion-based retries and attempt limits, with reopen tests. This
is injected diagnostic bookkeeping; authenticated SMTP delivery remains pending.

Owned-storage acceptance now composes real SMTP child I/O/timeout results with a
later certificate block. Reopen preserves the earlier retry deadline and attempt,
leaves blocked/untouched destinations unchanged, and retains state ownership.
These negotiation scenarios send no authenticated message.

STARTTLS diagnostics now distinguish a missing extension, temporary refusal, and
permanent refusal. Temporary refusals schedule durable retries; missing/permanent
cases stay pending for correction. Owned reopen fixtures verify the distinction,
and no refusal path writes TLS, credentials, or message commands.

A private AUTH PLAIN experiment now validates bounded ASCII credentials before
connecting, verifies required STARTTLS, and uses only fresh secure EHLO capabilities.
Loopback fixtures cover authentication replies, unsupported mechanisms, and trust,
hostname, and expiry failures before AUTH. It has no message submission, installed
credential frame, or worker authentication-result integration.
