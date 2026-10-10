# Slurm Notify

Slurm Notify is a planned user-space notification agent for researchers running
Slurm jobs. Written in Rust, it will discover the current user's jobs, detect
lifecycle events, and route notifications to configurable destinations without
requiring administrator privileges or changes to existing batch scripts.

**Status: repository foundation, pre-implementation.** A minimal Cargo workspace
and basic CI definition exist. The executable only reports that monitoring is
unavailable. No notifications or release binaries exist yet.
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

The executable prints its development status and exits with code 1. Fifty-seven Rust
tests exercise private scheduler parsing, command execution, lifecycle reconciliation,
SQLite commit/reopen recovery, and a mock-provider outbox worker with durable provider-requested retry delays. One subprocess helper is intentionally ignored
in the main suite and invoked by runner tests. Eight Python/SQLite schema tests and
six Git-hook tests run separately. The integrated pipeline persists deduplication
history and checkpoints, but remains test-only and is not exposed by the executable.
The proposed CLI commands are not implemented. See [contributor guidance](CONTRIBUTING.md)
and the [implementation plan](docs/architecture/implementation-plan.md).

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

The scaffold, minimal workspace, guidance, architecture decision, and basic CI
definition are present. Phase 0 still needs governance decisions and repository
configuration; hosted CI has not run.

## Development and validation

Ordinary development tests should run without a Slurm cluster using scheduler
fixtures, mock providers, temporary databases, and controlled failure scenarios.
Later CI gates will cover formatting, strict linting, unit/integration tests,
contract compatibility, dependency and secret checks, and supported-target
build/install smoke tests. Real-cluster tests require authorized resources and
compliance with local policy.

CLI, configuration, events, intentional Rust APIs, and persistence guarantees
must be governed explicitly. Contract baselines must be reviewed rather than
automatically rewritten to make checks pass.

## Open decisions

The handoff records unresolved decisions, including licensing, minimum Rust and
Slurm versions, Linux libc compatibility, provider release scope, default event
and catch-up behaviour, log privacy rules, remote authentication, and whether to
expose a supported Rust library API. Cluster polling intervals and background
execution permissions must be validated locally; the proposed 30-second polling
interval is not an approved operational default.

No license has been selected or granted; see [license decision placeholder](LICENSE.pending.md).
Rust 1.74.0 is pinned for foundation development only, not as a product support
baseline. Contract checks, security workflows, release automation, maintainer
ownership, and branch protection remain future work.
