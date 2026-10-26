# Dependabot compatibility review — 2026-10-26

Review of PRs 12–15 and 17 against the local MVP on main. The compiler remains
pinned to development Rust 1.88.0; no supported MSRV or platform matrix is declared.
Public CLI/config/event/persistence contracts and operational defaults are unchanged.

| PR | Update | Compatibility review |
| --- | --- | --- |
| [12](https://github.com/koros/slurm-notify/pull/12) | serde_json 1.0.152 → 1.0.154 | UTF-8 error-position fix and no_std/preserve_order support; static user diagnostics and strict config/public payload checks retained |
| [14](https://github.com/koros/slurm-notify/pull/14) | zeroize 1.8.1 → 1.9.1 | Requires Rust 1.85; Zeroizing Debug becomes opaque; retain secret-owning buffers and static diagnostics |
| [15](https://github.com/koros/slurm-notify/pull/15) | email-encoding 0.4.1 → 0.4.2 | Requires Rust 1.85, uses Base64 0.23; existing frozen SMTP rendering and recovery checks must pass |
| [13](https://github.com/koros/slurm-notify/pull/13) | idna_adapter 1.1.0 → 1.2.2 | Switches Unicode backend to ICU4X; resolved ICU 2.4 requires Rust 1.88 and adds Unicode data/derive dependencies |
| [17](https://github.com/koros/slurm-notify/pull/17) | taiki-e/install-action 2.87.27 → 2.87.31 | Verified upstream commit 1e29709c0ec63babbe8f266397a465f29a825f91; installer runtime/action definition and the two selected tool manifests unchanged |

## Boundaries and dependency growth

The lockfile adds ICU4X data, collection and derive crates and removes the older
idna_mapping/unicode-normalization backend. These are transitive runtime dependencies
through URL parsing despite the adapter's explicit pin being in dev-dependencies.
The local macOS optimized binary changed from 5,724,416 to 5,626,368 bytes
(toolchain/build-specific evidence, not an artifact-size promise). This expands
the dependency graph; it does not require a compiler change
from the existing Rust 1.88 baseline. Unicode-3.0 and the other resolved licenses
are already covered by the reviewed dependency policy; no exemptions are added.

The installed resolver still rejects non-ASCII URL input before authorization
lookup. Valid ASCII A-label/punycode hosts remain possible; malformed labels are
rejected. New regression acceptance checks safe-origin normalization, exact owned
endpoint bytes and changed-endpoint revision binding. Existing TLS verification,
credential rejection, redirects-off and retry/restart tests remain required.
This is not a general internationalized-domain compatibility guarantee.

`cargo tree --locked -e features -i base64@0.23.1` activates only std/alloc:
email-encoding does not enable the optional SIMD feature. Base64 0.22 remains
required by ureq. SMTP wire contracts are not regenerated for this update.

The installer update changes tool catalogs and upstream maintenance files, not
its action runtime or install script. Both workflow uses remain full-SHA pinned,
with cargo-llvm-cov 0.6.21/cargo-deny 0.20.2 and fallback disabled. Existing hosted
runner scope and permissions are retained; no release publication is added.

Upstream references: [serde_json releases](https://github.com/serde-rs/json/releases),
[zeroize changelog](https://github.com/RustCrypto/utils/blob/zeroize-v1.9.1/zeroize/CHANGELOG.md),
[email-encoding manifest](https://github.com/lettre/email-encoding/blob/v0.4.2/Cargo.toml),
[idna_adapter manifest](https://github.com/hsivonen/idna_adapter/blob/v1.2.2/Cargo.toml),
[installer comparison](https://github.com/taiki-e/install-action/compare/ab6895351a88b908e96a4697860d4ffea5e57dbe...1e29709c0ec63babbe8f266397a465f29a825f91).

## Validation

Full shared checks pass on Rust 1.88.0: 434 private Rust tests, five ignored child
helpers, 29 installed CLI/provider-codec tests, custom process/restart acceptance,
and 27 Python tests (eight SQLite, six hooks, 13 draft config). Rust line coverage
passes at 97.11% (27,425/28,242), above the unchanged 90% gate. Full-graph
advisory/license/source checks, Actionlint, strict rustdoc, optimized build and
help/version baseline smoke pass. Contract
baselines remain unchanged. Hosted checks must independently verify both platforms
and pinned tool installation; older PR runs alone do not prove this integration.


Dependabot advanced PR 17 to 2.87.31 after the initial 2.87.30 integration. The
additional reviewed diff only updates tool catalogs and its changelog; the runtime,
action definition and selected tool manifests remain unchanged.

The initial hosted integration passed all application tests but failed merging one
corrupt LLVM profile. The same failure appears in the earlier October 24 PR 13 run,
before these updates were integrated. The clean integration rerun passed both
platforms, coverage, reporting and artifact publication. It retains strict profile merging,
the complete test suite and the unchanged 90% threshold; no profile exclusions or
failure suppression are introduced. This transient instrumentation issue remains
a coverage-tooling reliability concern, not a proven dependency API incompatibility.
