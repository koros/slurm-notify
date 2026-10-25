# Contributing

Slurm Notify is an unreleased MIT-licensed prototype. Contributions that improve
reliability, privacy, diagnostics and operator documentation are welcome. Read
[AGENTS.md](AGENTS.md), the [current plan](docs/architecture/implementation-plan.md)
and relevant [ADRs](docs/adr/README.md) before changing behavior. Follow the
[Code of Conduct](CODE_OF_CONDUCT.md); report vulnerabilities through [SECURITY.md](SECURITY.md).

## Development setup

Use the pinned Rust 1.88.0 development toolchain, Python 3, Git and a native C
compiler for bundled SQLite. No minimum supported Rust version is declared.
The workspace currently has one package, `crates/notify-cli`; other crate folders
reserve possible future boundaries.

```sh
sh scripts/install-hooks.sh
sh scripts/check.sh
```

Ordinary tests need no Slurm installation or provider account. Synthetic scheduler
children and controlled local TLS/HTTP fixtures exercise normal, failure, signal
and restart behavior. Do not point fixtures at real accounts or clusters. Site
polling policy and explicit operational acceptance are separate from these tests.

## Required validation

`sh scripts/check.sh` runs formatting, strict Clippy, locked Rust and process tests,
Python checks, and debug build/CLI smoke checks. Pre-commit validates the staged
snapshot; pre-push runs shared checks. Fix failures; never bypass hooks or CI.

For Rust behavior or coverage configuration changes, install cargo-llvm-cov 0.6.21
and matching compiler tools, then run the 90% line-coverage gate:

```sh
rustup component add llvm-tools-preview --toolchain 1.88.0
cargo install cargo-llvm-cov --version 0.6.21 --locked
sh scripts/check-coverage.sh
```

Install cargo-deny 0.20.2 (or use its checksum-verified release binary) and check
advisories, dependency licenses and sources:

```sh
cargo deny --locked check
```

The allowlist in `deny.toml` includes the current dependencies' permissive code
licenses and CDLA-Permissive-2.0 for certificate-root data. It does not change this
project's MIT license. Review new licenses and advisories deliberately; do not
silence a finding with blanket exceptions. Duplicate transitive crate versions
are warnings, since compatible upstream dependency ranges can differ.

See [CI and coverage](docs/operations/ci-and-coverage.md) for report artifacts,
external services and check names. Hosted CI also validates workflows and rustdoc.

## Proposing changes

Keep increments focused. Describe the problem, resulting behavior, tests and
remaining limits. Update current docs and CHANGELOG.md with behavior changes;
put historical results in the implementation history. Use an ADR for durable
architectural decisions and proposals for unsettled public interfaces.

CLI, config, event and persistence changes need an explicit compatibility review.
Keep Rust internals private unless intentionally supporting a public API. Contract
baselines are reviewed evidence: do not regenerate them just to make checks pass.
Test meaningful failure/restart paths, especially where persisted data or delivery
semantics change. Prefer bounded subprocess argument arrays and explicit resource
ownership. Preserve verified TLS, privacy and durable recovery guarantees.

Use synthetic examples in issues and pull requests. Never attach secrets, webhook
URLs, authorization headers, real scheduler output or state databases. A cluster
compatibility report records evidence; it does not establish platform support.
Releases require explicit authorization and verified support/install acceptance.
