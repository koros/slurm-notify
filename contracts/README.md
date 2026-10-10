# Contract governance

These directories reserve reviewed baselines for CLI, configuration, events, and
any deliberately supported Rust library API. No baselines are approved yet.
The handoff examples remain proposals and must not be copied as final contracts.

Before introducing a public surface, document its semantics and version policy,
add machine-readable schemas or snapshots with representative fixtures, and obtain
explicit review. Future comparison tooling must report additions, removals, type
changes, and default changes against the approved baseline. It must never update
baselines automatically to pass CI.

Intentional changes require rationale, compatibility assessment, migration plan,
documentation, and a version decision. CODEOWNERS and protected-branch rules need
real maintainer identities and a configured GitHub repository; they are not
configured by this local scaffold. Rust API tooling is deferred until a supported
library API is deliberately exposed.
