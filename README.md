# Slurm Notify

[![CI](https://github.com/koros/slurm-notify/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/koros/slurm-notify/actions/workflows/ci.yml)
[![Coverage gate ≥90%](https://img.shields.io/badge/coverage_gate-%E2%89%A590%25-brightgreen)](docs/operations/ci-and-coverage.md)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Development Rust 1.88.0](https://img.shields.io/badge/development_Rust-1.88.0-orange?logo=rust)](rust-toolchain.toml)
[![Status: prototype](https://img.shields.io/badge/status-prototype-yellow)](docs/architecture/product-readiness.md)
[![CI platforms: Linux and macOS](https://img.shields.io/badge/CI_platforms-Linux%20%7C%20macOS-blue)](.github/workflows/ci.yml)

Slurm Notify is an MIT-licensed Rust project for user-space Slurm job notifications.
It aims to discover the current user's jobs and send lifecycle alerts without root
access or changes to batch scripts.

**Current status: tested private engine; installed monitoring is unavailable.**
The executable supports `--help`, `--version`, and
`config validate --config <absolute-path>`, and `status --config <absolute-path>`.
Status inspects existing historical state without resolving provider secrets. Empty or invalid
invocations exit 2; unapproved SMTP profiles exit 3. Monitoring commands remain unavailable. No supported
monitoring release is available yet. See the [installed configuration contract](contracts/config/README.md)
for file permissions, resource requirements and exact SMTP permission checks.

## Implemented

- Synthetic Slurm parsing, lifecycle reconciliation, filtering and routing.
- Durable SQLite checkpoints/outbox, deduplication, retries and frozen SMTP work.
- Transactional public webhook UUIDs, frozen payloads and endpoint revision bindings
  with guarded mixed-provider delivery, runtime cycles and owned continuous sessions
  in the private engine; installed sending remains pending.
- Supervised HTTPS webhook and verified required-STARTTLS SMTP transports.
- Protected config/trust snapshots, bounded Rust validation, explicit environment
  resources and effective-user lookup.
- Owned foreground sessions, continuous scheduling, SIGINT/SIGTERM cancellation and
  process tests for shutdown, partial delivery and restart recovery. Local continuous
  orchestration binds OS identity/environment and bounded synthetic scheduler children
  with explicit provider helper commands.
- Linux/macOS CI, pre-commit/pre-push checks and a 90% minimum Rust coverage gate.

These capabilities use synthetic scheduler data and controlled process/loopback
fixtures. See [product readiness](docs/architecture/product-readiness.md) for current
validation results and limits.

## Development

The workspace has one Cargo package, `crates/notify-cli`. Other crate directories
reserve future boundaries. Use the pinned Rust toolchain, Python 3 and a native C
compiler for bundled SQLite tests:

```sh
sh scripts/install-hooks.sh
sh scripts/check.sh
```

For coverage, install cargo-llvm-cov 0.6.21 and the pinned compiler's
llvm-tools-preview component, then run:

```sh
sh scripts/check-coverage.sh
```

See [contributing](CONTRIBUTING.md) and [CI/coverage details](docs/operations/ci-and-coverage.md).
CI artifacts contain the finite development CLI.

## Remaining work

The local MVP still needs installed continuous monitoring and delivery, setup/test/doctor flows, and permitted cluster/provider
acceptance. Configuration validation and offline historical status are installed;
continuous scheduling and owned-session recovery are tested internally. The intended MVP includes generic
webhook and SMTP delivery. Slack/Teams scope remains to be settled. Log analysis
and remote SSH monitoring are later phases.

The [current plan](docs/architecture/implementation-plan.md) records the next work.
The [contract review package](docs/proposals/local-contract-review-v1.md) records
adopted MVP interfaces and remaining acceptance. Configuration examples are synthetic
and require real protected resources and exact SMTP permissions; public event emission
remains unimplemented.
The [original design](SLURM_NOTIFY_DESIGN_AND_CODEX_HANDOFF.md),
[implementation history](docs/architecture/implementation-history.md) and
[ADRs](docs/adr/README.md) preserve design decisions and past evidence.

Product platform/Slurm support, cluster polling policy, security reporting and
release/install acceptance remain pending. A pinned development compiler and CI
platform matrix do not establish a supported deployment matrix.

## License

[MIT](LICENSE). Contributions are provided under the same license unless explicitly
agreed otherwise. Dependency notices retain their respective licenses.
