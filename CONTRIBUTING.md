# Contributing

This project is in its repository foundation phase. Read the design handoff and
[implementation plan](docs/architecture/implementation-plan.md) first.

## Local development

Install Rust through your normal development process. `rust-toolchain.toml` pins
Rust 1.74.0 for reproducible foundation checks; it does not establish an MSRV or a
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
