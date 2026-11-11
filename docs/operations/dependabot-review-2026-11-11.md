# Dependabot compatibility review — 2026-11-11

Integrate the updates proposed by PRs 28–31 into current main, retaining the
macos-15 runner correction. This is a source update, not a release or a new
platform/MSRV acceptance. Rust remains pinned to development 1.89.0.

| PR | Update | Scope and compatibility |
| --- | --- | --- |
| [28](https://github.com/koros/slurm-notify/pull/28) | lettre 0.11.22 → 0.11.23 | Rust 1.85; multiline SMTP error-reply fix and Base64 update. Only builder is enabled; the project's own verified SMTP boundary remains authoritative. |
| [29](https://github.com/koros/slurm-notify/pull/29) | libc 0.2.190 → 0.2.191 | Rust 1.65; platform binding additions and corrections, including Apple Mach availability. Existing process, signal and resource-ownership tests remain required. |
| [30](https://github.com/koros/slurm-notify/pull/30) | rusqlite 0.41.0 → 0.41.1 | Rust 1.89; zero-argument deterministic-function panic and SQLCipher pragma fixes. Bundled/hooks features stay unchanged; libsqlite3-sys remains 0.39.1 / SQLite 3.54.1. |
| [31](https://github.com/koros/slurm-notify/pull/31) | install-action 2.87.31 → 2.90.3 | Upstream tag resolves to cfdb7ed4ccc7c7dce37759eae89dc5674b469b39; both workflow uses remain full-SHA pinned. |

The installer runtime changes include distro/package detection, optional skip
fallback, cargo-binstall path selection and unrelated tool installation updates.
The selected cargo-deny 0.20.2 and cargo-llvm-cov 0.6.21 manifests are unchanged.
Our exact tool versions, checksum verification, fallback none, runner matrix and
workflow permissions remain unchanged. Optional skip behavior is not enabled.

Only the three requested Rust package versions/checksums change in Cargo.lock;
there are no additional packages or license-policy exemptions. Existing disabled
default features and scalar Base64 selection remain. No CLI/config/event baseline,
storage schema, operational default or credential handling changes are intended.
Do not regenerate contract baselines to accommodate dependency behavior changes.

Upstream references: [lettre release](https://github.com/lettre/lettre/releases/tag/v0.11.23),
[libc release](https://github.com/rust-lang/libc/releases/tag/0.2.191),
[rusqlite release](https://github.com/rusqlite/rusqlite/releases/tag/v0.41.1),
[installer comparison](https://github.com/taiki-e/install-action/compare/1e29709c0ec63babbe8f266397a465f29a825f91...cfdb7ed4ccc7c7dce37759eae89dc5674b469b39).

## Validation

Local and hosted results must be distinguished. Older PR runs had cancelled
macos-14 jobs; libc also had a failed coverage run as well as successful runs.
Those runs do not establish acceptance of the integrated current-main snapshot.
No live Slurm cluster or provider acceptance is performed by this review.
