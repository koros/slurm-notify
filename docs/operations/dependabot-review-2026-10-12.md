# Dependabot compatibility review — 2026-10-12

Review against current main and pinned development Rust 1.88.0. Public contracts,
installed placeholder behavior and product support decisions remain unchanged.

| PR | Update | Compatibility result |
| --- | --- | --- |
| [10](https://github.com/koros/slurm-notify/pull/10) | url 2.5.8 | Existing URL validation/transport tests pass |
| [8](https://github.com/koros/slurm-notify/pull/8) | time 0.3.55 | Builds on existing Rust 1.88.0; no compiler increase |
| [7](https://github.com/koros/slurm-notify/pull/7) | powerfmt 0.2.1 | No source migration |
| [5](https://github.com/koros/slurm-notify/pull/5) | base64 0.23.1 | Existing codec API works; disable new default simd-unsafe feature |
| [4](https://github.com/koros/slurm-notify/pull/4) | rusqlite 0.40.2 | Version-only update fails: usize FromSql removed; decode two health counts as i64 and checked-convert |
| [9](https://github.com/koros/slurm-notify/pull/9) | rcgen 0.14.10 | Version-only update fails: migrate fixture certificate/signing-key APIs and fallible parameters |
| [1](https://github.com/koros/slurm-notify/pull/1) | checkout 7.0.1 | Inputs/events remain compatible; verify upstream tag SHA and Node 24 runner requirement |
| [2](https://github.com/koros/slurm-notify/pull/2) | upload-artifact 7.0.2 | Existing directory/archive/retention/error inputs remain compatible; Node 24 |
| [6](https://github.com/koros/slurm-notify/pull/6) | dependency-review-action 5.0.0 | Existing severity/license inputs remain compatible; Node 24; PR-only job requires hosted PR evidence |

## Migrations and dependency bounds

All six direct Cargo pins are updated with a locked build. Bundled SQLite uses
libsqlite3-sys 0.38.2. rcgen uses yasna 0.6.0 and existing ring, with default features
disabled because fixtures only need DER. This also removes the PEM feature's
transitive activation of Base64 SIMD. `cargo tree -e features -i base64@0.23.1`
shows only std/alloc. Base64 0.22.1 remains legitimately required by other existing
dependencies. Optional ASN.1/X.509 packages are in the lock but not activated.

TLS fixtures still use fresh in-memory keys. Tests retain expired/untrusted/wrong
identity rejection, AUTH ordering, final acknowledgments, deadline/restart and
frozen-message behavior. Storage count conversion fails rather than truncates.

Actions stay pinned to verified full upstream tag commits:

- checkout: `3d3c42e5aac5ba805825da76410c181273ba90b1`.
- upload-artifact: `cf430e030ddbb5b0abf93d22962f4752f3646cd9`.
- dependency-review-action: `a1d282b36b6f3519aa1f3fc636f609c47dddb294`.

These actions use Node 24 and require a sufficiently recent runner (at least
2.327.1). CI uses GitHub-hosted Ubuntu/macOS; this does not establish self-hosted
runner compatibility. The new checkout fork protections affect pull_request_target/
workflow_run, neither of which this workflow uses. Existing upload archive defaults
and missing-file/retention policies are retained.

Upstream references: [checkout](https://github.com/actions/checkout/tree/v7.0.1),
[upload artifact](https://github.com/actions/upload-artifact/tree/v7.0.2),
[dependency review](https://github.com/actions/dependency-review-action/tree/v5.0.0).

## Validation

Shared local checks pass: 322 Rust tests plus one ignored helper, custom process
acceptance, eight SQLite, six hook and 11 draft configuration tests. Rust line
coverage is 97.59% (18,509/18,966). Checksum-verified Actionlint 1.7.7 passes.

Earlier upload/review PR checks failed on old workflow tool installation and
repository dependency-graph availability, not an observed action-input API break.
Their stale-base failures do not establish current compatibility. Hosted push
checks must confirm platform execution and artifact uploads; dependency review
only executes for pull_request events. No failed gate is bypassed or disabled.
