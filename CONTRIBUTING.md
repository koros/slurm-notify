# Contributing

This project is in its repository foundation phase. Read the design handoff and
[implementation plan](docs/architecture/implementation-plan.md) first.

## Local development

Install Rust through your normal development process. `rust-toolchain.toml` pins
Rust 1.88.0 for reproducible foundation checks; it does not establish an MSRV or a
supported deployment platform. The workspace currently has one executable. The test-only storage prototype uses
rusqlite with bundled SQLite; running Rust tests requires a native C compiler.
There are no runtime dependencies in the placeholder executable.

```sh
cargo build --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 -m unittest discover -s tests/integration/storage -p 'test_*.py' -v
```

The executable is a development placeholder and exits with status 1. No commands,
flags, configuration, or notification functionality are supported yet. The test harness exercises a private, test-only lifecycle prototype using synthetic
accounting fixtures; it does not run scheduler commands or deliver notifications.

## Local Git hooks

Install the version-controlled hooks once per clone:

```sh
sh scripts/install-hooks.sh
```

The installer sets this repository's local `core.hooksPath` to `.githooks` and
preserves an existing custom hooks path by refusing to overwrite it. Git, Cargo,
Rustup, Python 3, and a native C compiler for the SQLite test dependency are required.
There is no separate hook manager to install.

Pre-commit checks staged files for common credential patterns, credential filenames,
merge markers, and trailing whitespace. Markdown's two-space hard breaks are
allowed. It exports the staged index to a temporary directory and runs formatting,
strict Clippy, Rust tests, SQLite tests, hook tests, build, and executable smoke
checks there. Unstaged changes cannot mask failures in committed content. Symlinks
and submodules are unsupported by this snapshot check and are rejected explicitly.
Temporary files are removed afterward; build cache stays under `target/hooks`.

Pre-push checks tracked index files and runs the same suite against the working
tree. It does not validate each historical commit or every ref named in a push.
CI independently runs the same suite for the checked-out commit; contributors
must install local hooks themselves because Git does not activate them on clone.
The credential patterns are a small guardrail, not a complete credential scanner.

Run the shared suite manually with:

```sh
sh scripts/check.sh
```

Keep commit messages descriptive; there is no mandatory message format or
commit-message hook. Branch protection and required reviews must be configured
on GitHub separately; this setup does not change remote repository settings.

## Changes and review

Keep changes focused and explain the behaviour, rationale, and validation in each
pull request. Add fixtures under `tests/fixtures`, keep credentials out of samples,
and use mocked scheduler/provider dependencies for ordinary tests. Add unit tests
inside the owning crate and integration tests under its Cargo-recognized `tests/`
directory; the root `tests/integration` and `tests/e2e` directories are reserved
for future shared tooling and do not run automatically with Cargo.

Public contract changes need explicit approval, compatibility impact, migration
instructions where relevant, documentation, and an appropriate version decision.
See `contracts/README.md`. Use descriptive commits; no commit format is mandated.

License and contribution governance are undecided. Resolve these before accepting
external contributions or publishing a release.

The SQLite schema experiment uses Python 3's standard-library `sqlite3`; Cargo does
not run these tests. The CI workflow invokes them separately. No Python runtime
requirement is introduced for the eventual notification executable.

## CI and coverage

CI now checks Linux and macOS, documentation, workflow syntax, and pull-request
dependency vulnerabilities, and enforces at least 80% aggregate Rust line coverage.
Run `sh scripts/check-coverage.sh` after installing cargo-llvm-cov 0.6.21 and the
pinned compiler's llvm-tools-preview component. Coverage tools are optional for
local hooks and mandatory in CI. Reports and development artifact scope are
explained in [CI and coverage](docs/operations/ci-and-coverage.md).
