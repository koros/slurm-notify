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

**Current status: the local MVP command set is implemented; this is an unreleased prototype.**
It includes interactive `init`, `config validate`, `doctor`, explicit provider
`test`, foreground `start`, historical `status`, help and version. Controlled
acceptance covers webhook, SMTP and mixed delivery, verified TLS, signals and
durable restart from a relocated executable.

For Slack incoming webhooks, use the [HPC Slack setup guide](docs/operations/slack.md).

Start with the [getting-started guide](docs/operations/getting-started.md).
The [CLI contract](contracts/cli/README.md) and
[configuration contract](contracts/config/README.md) define exact behavior.
Validate site polling/execution policy before real-cluster monitoring. The
[support and acceptance gates](docs/operations/support-and-acceptance.md) distinguish
local implementation from a supported deployment/release.

## Implemented

- Synthetic Slurm parsing, lifecycle reconciliation, filtering and routing.
- Durable SQLite checkpoints/outbox, deduplication, retries and frozen SMTP work.
- Transactional public webhook UUIDs, frozen payloads and endpoint revision bindings
  with guarded mixed-provider delivery, runtime cycles and owned continuous sessions
  in the shared engine; installed SMTP submission/restart has controlled TLS acceptance.
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
CI artifacts contain the development prototype CLI.

## Remaining acceptance

Authorized real-cluster/provider acceptance, platform/Slurm support baselines,
operator installation/upgrade review and explicit release approval remain. See
[product readiness](docs/architecture/product-readiness.md) and the
[current plan](docs/architecture/implementation-plan.md). Generic webhook and SMTP form the original local MVP scope; an explicit Slack
incoming webhook adapter is now implemented with controlled acceptance. Teams,
log analysis and remote SSH remain later work.

The [contract adoption package](docs/proposals/local-contract-review-v1.md) records
reviewed interfaces. Examples are synthetic and require protected resources and
explicit SMTP enrollment. The [original design](SLURM_NOTIFY_DESIGN_AND_CODEX_HANDOFF.md),
[implementation history](docs/architecture/implementation-history.md) and
[ADRs](docs/adr/README.md) preserve design decisions and historical evidence.
See [security reporting](SECURITY.md) before sharing diagnostics.

## License

[MIT](LICENSE). Contributions are provided under the same license unless explicitly
agreed otherwise. Dependency notices retain their respective licenses.
