# Current implementation plan

The local MVP command set is implemented. It remains an unreleased prototype,
with controlled acceptance rather than a supported deployment promise.
[Product readiness](product-readiness.md) records current evidence and limits;
[implementation history](implementation-history.md) preserves historical increments.

## Completed local scope

- Resource-free help/version and deliberate CLI/config/event contract baselines.
- Interactive protected draft review, safe provider/disclosure previews, explicit
  current SMTP pair enrollment and atomic no-overwrite configuration creation.
- Protected configuration/resource validation and read-only doctor with bounded
  scheduler version probes; no hidden network/state activity.
- Explicit one-shot synthetic HTTPS/SMTP tests with preview and no automatic retry.
- Effective-user local foreground monitoring with generic HTTPS webhook, verified
  required STARTTLS SMTP and mixed routes; no background service or public Rust API.
- Atomic public UUID/frozen payload/SMTP preparation, owned private SQLite state,
  retry and pending-resource guards, bounded maintenance and durable restart.
- SIGINT/SIGTERM handling, sampled health and offline historical status.
- Relocated-executable terminal setup/validation/doctor/mixed-delivery acceptance,
  partial completion, identical HTTPS replay and no replay of completed SMTP work.
- MIT licensing, hooks, shared checks and a minimum 90% Rust line coverage gate.
- User-focused setup/status documentation, private security/conduct reporting, issue
  forms, dependency policy checks and external coverage/Scorecard publication.

The local contracts were adopted on 2026-10-13. Recent decisions are recorded in
[ADRs 0065–0071](../adr/README.md), including protected trust, doctor, provider tests,
setup and relocated workflow acceptance. The
[getting-started guide](../operations/getting-started.md) documents the implemented flow.

## Remaining support and release gates

The operator confirmed protected configuration creation and validation on GPFS
with the [portable publication fallback](../adr/0072-portable-no-overwrite-configuration-publication.md).
Gmail SMTP connectivity failed before authentication. The operator confirmed
installed synthetic Slack delivery, healthy monitoring after correcting cluster
identity, and completed array-task notifications. Local cluster preflight now
rejects mismatches explicitly; detailed Slack rendering and v6 frozen-message state
have controlled tests and need the next HPC acceptance. The short per-command PATH
workaround remains documented. See [Slack setup/upgrade](../operations/slack.md).
Sync failures remain fatal. No supported deployment/release is implied.

1. Obtain explicit authorization and site policy for real-cluster/provider
   acceptance. Establish Slurm/accounting/job-identity/timezone baselines from
   evidence; preserve privacy and current-user filtering.
2. Verify intended target/platform/filesystem installation, termination, migration,
   closed backup and rollback behavior. Local/CI tests do not define an MSRV/libc
   or supported platform matrix.
3. Complete maintainer interface/security/dependency-license and repository
   protection review, then approve an artifact/release plan. Publishing requires
   explicit authorization and acceptance of the exact installation artifacts.

See [the acceptance checklist](../operations/support-and-acceptance.md).
Slack incoming webhook rendering is implemented as an explicit extension
([ADR 0073](../adr/0073-slack-incoming-webhook-rendering.md)); Teams-specific
integrations, log monitoring, remote SSH and background service installation remain
outside the implemented local MVP. No release is published.

## Notification presentation milestone

Tracking: [GitHub milestone 1](https://github.com/koros/slurm-notify/milestone/1).

Adopt a private shared presentation model (heading, fields, context and complete
plain-text fallback), with explicit platform renderers. Preserve per-destination
privacy choices, frozen retries, payload budgets and generic webhook contracts.

- [x] Introduce the private presentation model and Slack Block Kit v3 renderer for
  opted-in detailed messages, including synthetic tests and readable lifecycle headings.
- [x] Keep original minimal Slack v1 and generic webhook/SMTP bytes unchanged;
  bind v3 to a new revision and reject switching pending work from older renderers.
- [x] Complete controlled full-suite, restart and 90% coverage acceptance
  (97.17% Rust line coverage locally).
- [ ] Operator acceptance of the new layout on the HPC after draining older work.
- [ ] Discord: review exact config/contract, implement embeds and explicit mention
  suppression, provider acknowledgement/rate limits, privacy and restart acceptance.
- [ ] Teams: review Workflow setup/authentication/ownership and exact config/contract,
  implement Adaptive Cards, provider acknowledgement/rate limits and restart acceptance.

Discord and Teams are planned; this milestone adds no provider support for them.
See [ADR 0076](../adr/0076-slack-block-kit-presentation.md).

## Validation

`sh scripts/check.sh` is the shared local/CI suite; staged and pre-push hooks must pass.
`sh scripts/check-coverage.sh` enforces at least 90% aggregate Rust line coverage.
Record current results in product readiness, without appending obsolete status to
README. Verify hosted checks separately from local results.
