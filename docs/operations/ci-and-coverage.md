# CI and coverage

GitHub's CI workflow runs on pushes, pull requests, and manual dispatch. It checks
tracked-file guardrails, formatting, strict Clippy, locked Rust tests (including
the custom child-process harness), SQLite, hook and private draft-configuration
fixture tests, build/smoke behavior,
and rustdoc on Linux and macOS. Actionlint validates workflow syntax. Pull requests
also receive dependency review, failing on newly introduced moderate-or-higher
vulnerabilities. Dependency review handles PR diffs; the separate cargo-deny workflow
checks the complete locked graph for advisories, licenses and sources. Dependabot opens weekly Cargo and GitHub Actions updates;
compiler compatibility and existing dependency pins still require review.

## Coverage gate

The coverage job fails below **90% aggregate Rust workspace line coverage** using
cargo-llvm-cov 0.6.21 and LLVM tools matching the pinned Rust 1.88.0 compiler.
The prebuilt coverage tool keeps the coverage installation reproducible. Third-party Actions are commit-pinned;
installation disables fallback and retains checksum verification.

Run locally from the repository root:

```sh
rustup component add llvm-tools-preview --toolchain 1.88.0
# Install cargo-llvm-cov 0.6.21 using its upstream prebuilt installation instructions.
sh scripts/check-coverage.sh
```

See the [upstream tool instructions](https://github.com/taiki-e/cargo-llvm-cov/tree/v0.6.21).
Reports are written to `target/coverage/rust.json`, `lcov.info`, and `html/` before
threshold enforcement; CI retains them as `rust-coverage` for 14 days, including
when the threshold fails. A failed test fails the job even without a report.
A separate main-push job uploads the measured LCOV artifact to Codecov using GitHub
OIDC, without a stored upload token. It does not execute repository tests with write
permissions. External upload errors are non-blocking: the independent 90% gate
remains required regardless of reporting. Main reports are uploaded even when the
threshold fails, provided the coverage job produced an LCOV artifact. The README badge shows Codecov's
latest uploaded main measurement, which may lag the current commit or be unavailable
until service onboarding succeeds; it is not a green threshold badge.

Current local counts and coverage are recorded in
[product readiness](../architecture/product-readiness.md). Counts can vary between
platforms/runs; hosted results must be checked separately.
The denominator is cargo-llvm-cov's default workspace report: crate source,
including embedded unit-test/fixture code. Dependency code, bundled SQLite C,
Python/shell tooling, and integration harness source files excluded by the tool's
default filename filters are outside this Rust percentage. The custom process
harness still executes and contributes coverage for included crate source. The
script collects fallback LLVM profiles from successfully exited supervised children
whose environment is deliberately cleared; killed children may produce no profile.
Before each run, cargo-llvm-cov removes its prior raw profiles, and the script
removes crate fallback profiles. This measures the current run without accumulating
previous runs or mixing LLVM formats after a compiler upgrade.
There are no project-specific filename exclusions or suppressed uncovered modules.
This metric describes the current Rust implementation; it does not certify working product
notifications. Python and shell behavior have separate CI checks, not a claim of
90% measured coverage. Coverage is authoritative in CI; local Git hooks retain
their existing shared checks without requiring contributors to install LLVM tools.

## Build artifacts and release boundary

Successful main pushes produce a checksummed Linux x86_64 **development CLI**
artifact after platform checks, coverage, and workflow validation pass. It is
retained for seven days and smoke-tested against approved help/version snapshots
(exit 0) and empty-invocation usage (exit 2). This artifact is not a supported notification binary or a release.
Release publication and deployment are not automated while public contracts and release acceptance remain unresolved. Development
artifacts include the MIT LICENSE; dependency licenses are checked by the separate policy workflow.

Repository branch protection is a separate GitHub administrator setting. To enforce
these checks before merging, require `Checks (ubuntu-22.04)`, `Checks (macos-14)`,
`Rust coverage (at least 90%)`, `Workflow validation`, `Advisories, licenses and sources`,
and PR-only `Dependency review` in a
ruleset. Workflow files alone do not prevent bypass via direct pushes. This change
does not alter remote branch-protection settings.

## Draft configuration fixtures

The shared suite runs the Python standard-library harness in
`tests/integration/config_drafts`. It checks draft example shape and proposed
budgets with failure/edge fixtures, without loading referenced files or secrets.
It is not an installed validator, approved schema, or part of the Rust coverage
percentage. Draft policy data remains under `docs/proposals`; approved contracts
are not regenerated by these checks.

## Dependabot compatibility review

The [2026-10-12 review](dependabot-review-2026-10-12.md) records the nine proposed
updates, Rust fixture/count migrations and validation. Checkout 7.0.1,
upload-artifact 7.0.2 and dependency-review-action 5.0.0 are full-SHA pinned and use
Node 24; sufficiently recent runners are required. Existing hosted runners remain
the CI scope. PR-only dependency review must be checked on a PR, not inferred from
a successful push workflow.

## Dependency policy and Scorecard

`dependencies.yml` runs on pushes, PRs, weekly schedules and manual dispatch. Pinned
cargo-deny 0.20.2 checks the locked graph against RustSec, including unmaintained,
unsound and yanked packages, and the explicit license/source policy in `deny.toml`.
The allowlist includes permissive code licenses and CDLA-Permissive-2.0 for root
certificate data; this does not relicense dependencies or the MIT project.
Duplicate versions warn; unknown registries/git sources and wildcard versions fail.
No advisory exceptions are configured. Checks need network access to the advisory DB.

`scorecard.yml` runs on main pushes and weekly schedules with a read-only contents
token. Only its analysis job receives OIDC write permission to publish results to
OpenSSF's public API, which supplies the live Scorecard badge. SARIF is retained
as an artifact for 14 days. This workflow does not upload GitHub code-scanning
alerts or promise a particular score. New services may show unavailable until their
first successful publication. Third-party Actions are pinned to reviewed commits.
