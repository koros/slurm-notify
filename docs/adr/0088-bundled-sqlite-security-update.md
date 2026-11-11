# ADR 0088: bundled SQLite security update

Status: accepted for private development; deployment acceptance remains separate.

## Decision and evidence

The supplied dependency-policy log rejects libsqlite3-sys 0.38.2 under
RUSTSEC-2026-0459 (CVE-2026-119375). The advisory describes disclosure of
uninitialized page-cache memory when opening a crafted database/WAL pair with
mismatched page sizes. Existing private ownership and sidecar checks remain
required; they do not justify retaining a vulnerable bundled library.

Pin rusqlite 0.41.0 with the existing disabled defaults and bundled/hooks features.
The lockfile resolves libsqlite3-sys 0.39.1, bundling SQLite 3.54.1, above the
reported patched threshold 0.39.0 / SQLite 3.54.0. No advisory ignores or license
allowance changes are introduced. Duplicate-base64 and unused-license warnings
are separate from the advisory failure and do not justify unrelated graph changes.

The published rusqlite 0.41.0 Cargo manifest declares Rust 1.89.0 (confirmed by
`cargo info rusqlite@0.41.0`), so development, CI, contributor instructions and
coverage LLVM tools move together to 1.89.0. This supersedes the development pin
in ADR 0009 and establishes no public MSRV or platform support promise.

References: [upstream release](https://github.com/rusqlite/rusqlite/releases/tag/v0.41.0),
[published package](https://crates.io/crates/rusqlite/0.41.0),
[advisory](https://rustsec.org/advisories/RUSTSEC-2026-0459).

## Compatibility and validation

The updated hook API requires a mutable connection when installing/removing the
progress callback. Adapt the private query-budget helper and its callers; the
existing interruption/reset test verifies the same timeout behavior.

Only the SQLite wrapper and bundled native library change in Cargo.lock. No new
operational defaults, schema migration, CLI/config/event baseline or public Rust
API is introduced. Retain protected state, closed backups and existing recovery
rules. Ordinary validation uses synthetic scheduler data and controlled providers;
no real cluster or provider is contacted. Hosted checks and intended-node acceptance
remain separate from local validation.

Local validation on 2026-11-11: cargo-deny 0.20.2 `cargo deny --locked check`
passes advisories/bans/licenses/sources; existing duplicate-base64 and unused-Zlib
allowance warnings remain. Shared formatting, strict Clippy, Rust unit/process,
Python and build/smoke checks pass on Rust 1.89.0. The system Python was selected
with `PATH="/usr/bin:$PATH" sh scripts/check.sh`: the default framework Python
has group-writable ancestors and is correctly rejected by unattended preparation.
The final coverage run passes at 96.35% Rust lines (90% gate unchanged).
