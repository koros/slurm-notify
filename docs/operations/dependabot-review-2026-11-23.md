# Dependabot compatibility review — 2026-11-23

Integrate PRs 33–38 against current main. Development Rust remains 1.89.0;
no supported MSRV, platform or release acceptance is established. Existing local
feature work is excluded from the dependency commit.

| PR | Update | Review |
| --- | --- | --- |
| [33](https://github.com/koros/slurm-notify/pull/33) | email-encoding 0.4.2 → 0.4.3 | Rust 1.85. Encoding selection handles NUL/bare line endings and folding avoids whitespace-only lines. Existing SMTP wire/restart tests must pass unchanged. |
| [34](https://github.com/koros/slurm-notify/pull/34) | dependency-review-action 5.0.0 → 5.0.1 | Verified commit bbc44d14a204704ccc493733616cd1e93a0321ec. Runtime/dependency maintenance and resolved-comment handling; retain moderate severity gate, separate cargo-deny license enforcement and read-only permissions. PR comments remain disabled by the action default. |
| [35](https://github.com/koros/slurm-notify/pull/35) | install-action 2.90.3 → 2.90.6 | Verified commit 75a7c0f87bd666af3cdb75c5b9e9d03ab6044e6f. Catalog-only changes; main.sh/action.yml unchanged. Keep cargo-deny 0.20.2, cargo-llvm-cov 0.6.21, checksum validation and fallback none. The existing 0.20.2 catalog entry remains unchanged despite a new latest alias. |
| [36](https://github.com/koros/slurm-notify/pull/36) | rusqlite 0.41.1 → 0.43.0 | Rust 1.89. Changes include empty/comment-only SQL rejection and fixes to optional session/vtab/metadata APIs. Default features stay disabled; bundled/hooks remain enabled. libsqlite3-sys moves to 0.40.0, still bundling SQLite 3.54.1. No schema or journal-policy change. |
| [37](https://github.com/koros/slurm-notify/pull/37) | zeroize 1.9.1 → 1.9.2 | Rust 1.85. Optimizes byte/slice clearing and fallback optimization barriers; existing secret-owning buffers and opaque diagnostics remain. This adds no guarantee of erasing copies in libraries, files or SQLite pages. |
| [38](https://github.com/koros/slurm-notify/pull/38) | scorecard-action 2.5.0 → 2.6.0 | Verified peeled tag commit 23f22cd43608bae7a1f1fa2603d7314dee9538a5 (annotated tag object 56eaeac26fc540dfface1f07f79b9426f1956b4d). Publication now reports status/outputs. Set fail_on_publish_error true so permanent API rejection fails; upstream retries transient network/408/429/5xx failures and warns. A successful scan is not proof of publication. |

## Additional required security updates

Baseline cargo-deny failed for lettre 0.11.23 (RUSTSEC-2026-0560 through 0565)
and url 2.5.8 (RUSTSEC-2026-0592). Upgrade to exact lettre 0.11.24 and url 2.5.9
rather than suppressing advisories. Their declared compiler requirements are 1.85
and 1.63 respectively. URL requires form_urlencoded 1.2.3; retaining 1.2.2 is not
compatible with its manifest. No unrelated packages are updated or added.

Lettre's SMTP transport is disabled: we use only its builder, with our own bounded
verified STARTTLS, reply parsing and dot transparency. Our strict ASCII addr-spec
validation excludes display names and quoted local parts, limiting exposure to the
reported mailbox issues. Upgrade regardless; do not infer that every upstream
advisory is reachable in this application. SMTP injection and immutable replay
acceptance remain required.

Webhook validation rejects unsupported schemes and over-budget endpoints before
reading authorization. Extend that regression with single/nested blob URLs,
including the advisory-sized 10,000-prefix input; it must reject with the static
Endpoint diagnostic without authorization lookup or network activity. Existing
ASCII HTTPS safe-origin behavior and exact endpoint revision binding remain.

No new runtime defaults, dependency packages, TLS exceptions, advisory/license ignores,
CLI/config/event baselines or persistence schema changes are introduced. All action
references remain full-SHA pinned. Scorecard keeps its existing permission scope
and version-tagged upstream Docker image; pinning the action commit does not make
that upstream image reference an immutable digest. Publishing a release remains
separate and requires explicit authorization.

Upstream sources: [rusqlite 0.43](https://github.com/rusqlite/rusqlite/releases/tag/v0.43.0),
[rusqlite 0.42](https://github.com/rusqlite/rusqlite/releases/tag/v0.42.0),
[zeroize changelog](https://github.com/RustCrypto/utils/blob/zeroize-v1.9.2/zeroize/CHANGELOG.md),
[email-encoding source](https://docs.rs/crate/email-encoding/0.4.3/source/),
[lettre security release](https://github.com/lettre/lettre/releases/tag/v0.11.24),
[URL fix](https://github.com/servo/rust-url/pull/1186),
[installer comparison](https://github.com/taiki-e/install-action/compare/cfdb7ed4ccc7c7dce37759eae89dc5674b469b39...75a7c0f87bd666af3cdb75c5b9e9d03ab6044e6f),
[dependency review comparison](https://github.com/actions/dependency-review-action/compare/a1d282b36b6f3519aa1f3fc636f609c47dddb294...bbc44d14a204704ccc493733616cd1e93a0321ec),
[Scorecard comparison](https://github.com/ossf/scorecard-action/compare/fa7117db83c52d787ae5a89f350999701505d78e...23f22cd43608bae7a1f1fa2603d7314dee9538a5).

## Validation scope

PR checks are evidence for their separate heads, not for this combined graph.
Most supplied heads had advisory failures; some also had unsuccessful coverage
runs alongside successful runs. Ordinary local checks use synthetic scheduler data
and controlled providers. Hosted Linux/macOS, coverage, installer and Scorecard
publication must be independently verified for the integrated commit. No live
cluster, provider or installed-platform acceptance is performed by this increment.

Local validation on 2026-11-23: shared formatting, strict Clippy, locked Rust
unit/CLI/process/restart, Python and build/smoke checks pass with the protected
system Python selected via PATH. Strict rustdoc and checksum-verified Actionlint
1.7.7 pass. Cargo-deny 0.20.2 passes advisories/bans/licenses/sources without
policy changes; the existing duplicate Base64 and unused Zlib allowance warnings
remain. Workspace Rust line coverage is 96.35% (90% gate unchanged), including
pre-existing uncommitted local features; staged-snapshot hooks independently
validate the focused commit. Hosted results remain separate until observed.

## Scorecard deployment correction — 2026-11-24

The screenshot commit's Scorecard run failed before checkout or scanning because
GitHub runners could not pull `ghcr.io/ossf/scorecard-action:v2.6.0`
(`manifest unknown`). A rerun reproduced the failure even though an anonymous
manifest query from the development host returned HTTP 200. That response is not
proof the runner can pull the complete image. Restore the previously working
full-SHA v2.5.0 action pin and remove the v2.6-only fail_on_publish_error input.
Version 2.5 retains its original fatal publication-error handling; permissions,
publication enablement and SARIF artifact checks remain unchanged. Defer PR 38's
upgrade until the exact image is usable on the hosted runner. All Rust/security
and other action updates from this review remain applied.
