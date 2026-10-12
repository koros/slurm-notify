# Contract governance

These directories reserve reviewed baselines for CLI, configuration, events, and
any deliberately supported Rust library API. The [finite help/version CLI](cli/README.md)
is approved. The local MVP configuration, event, SMTP, CLI and limits proposals
were adopted on 2026-10-13; schema/snapshot baselines and implementation acceptance
remain pending for unfinished surfaces. [Webhook-only configuration validation](config/README.md)
has an installed structural schema and CLI snapshots. No public Rust library API is approved.
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

Current review drafts are collected in the [local contract review package](../docs/proposals/local-contract-review-v1.md), including CLI, configuration, SMTP and events.
They live in [docs/proposals](../docs/proposals):
[webhook payload](../docs/proposals/webhook-payload-v1.md) and
[local webhook configuration](../docs/proposals/local-webhook-config-v1.md).
Their examples are illustrative, not schemas, compatibility baselines, or supported
configuration. Keep draft revisions separate from approved contracts.
