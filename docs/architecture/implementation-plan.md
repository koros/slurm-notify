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

The local contracts were adopted on 2026-10-13. Recent decisions are recorded in
[ADRs 0065–0071](../adr/README.md), including protected trust, doctor, provider tests,
setup and relocated workflow acceptance. The
[getting-started guide](../operations/getting-started.md) documents the implemented flow.

## Remaining support and release gates

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
Slack/Teams-specific integrations, log monitoring, remote SSH and background service
installation are outside the implemented local MVP. No release is published.

## Validation

`sh scripts/check.sh` is the shared local/CI suite; staged and pre-push hooks must pass.
`sh scripts/check-coverage.sh` enforces at least 90% aggregate Rust line coverage.
Record current results in product readiness, without appending obsolete status to
README. Verify hosted checks separately from local results.
