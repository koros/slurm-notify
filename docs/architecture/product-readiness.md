# Product readiness assessment

Assessment after controlled detailed Slack, cluster preflight and relocated mixed workflow acceptance.
The private OS/runtime lifecycle and continuous owned sessions are also tested.
Historical increments remain in the implementation history; this assessment groups
current evidence by capability and identifies the remaining activation gates.

## Verified prototype

- Synthetic Slurm command parsing, bounded subprocess execution, lifecycle
  reconciliation and durable poll/checkpoint recovery.
- SQLite ledger/outbox, atomic SMTP preparation, immutable rendering/provider
  bindings, completion-based retries, retention and bounded revision cleanup.
- Supervised webhook transport and required STARTTLS SMTP submission with verified
  TLS identity, explicit trust roots, AUTH identity and frozen message bytes.
- Caller-driven mixed-route ticks, explicit permissions, downgrade protection,
  cancellation, static resolution failures, partial progress and reopen recovery.
- Protected bounded configuration/trust snapshots, owned injected SMTP credentials
  and webhook resources, typed operational controls and injected identity/policy mapping.
- Event-specific selection, atomic selected SMTP preparation and mixed runtime-cycle
  acceptance with edited rules, unmatched-event deduplication and restart recovery.
- Shared local checks, staged/pre-push hooks and a Rust coverage gate of at least 90%.

These results use synthetic scheduler data, temporary owned state and controlled
process/loopback fixtures. They establish controlled installed prototype behavior, not supported
cluster compatibility or a supported monitoring release. Implicit TLS in
the bound supervised SMTP path remains blocked.

## Evidence and limits

The latest implementation milestone passed 433 private Rust tests plus twenty installed CLI acceptance tests
and nine provider-codec checks and five ignored child helpers,
custom process acceptance, eight SQLite tests, six hook tests and 13 draft-config
Python tests. Rust line coverage was 97.11% (27,396/28,212); the gate is 90%.
The optimized build and help baseline smoke check also passed. Hosted checks are verified separately from these local results.

| Capability | Current evidence | Remaining gap |
| --- | --- | --- |
| Slack incoming webhook | Status icons, opt-in accounting details/names, per-destination frozen v6 messages, rollback and identical retries; installed cluster mismatch rejected before state opens | Operator confirmed v1 synthetic delivery and array-task completion alerts; v2 details/icons need HPC acceptance |
| Protected resources | Installed validation and webhook/SMTP monitoring reuse protected configuration/trust snapshots and exact SMTP permissions | Bounded synchronous account lookup |
| State ownership | Installed status and foreground runs enforce exclusive ownership; signal shutdown releases state | Live status access and installation acceptance |
| Routing and recovery | Public event transactions, mixed admission, blocked-work preservation and restart recovery are tested | Authorized real-provider acceptance |
| Foreground runtime | Installed SMTP/mixed runs cover scheduler processes, cadence, signals, health, bounded cleanup and restart without replaying completed mail | Supported cluster and platform acceptance |
| Provider transport | Selected same-executable helpers enforce HTTPS or verified STARTTLS; controlled installed HTTPS/SMTP submission and mixed restart succeed | Authorized real-provider acceptance |

Cancellation is cooperative at stage boundaries. Synchronous account lookup and
filesystem operations do not acquire a universal deadline from the foreground
scheduler. Controlled loopback fixtures establish no real-provider acceptance.

## Remaining support and release acceptance

The implemented local CLI includes init, config validation, doctor, provider test,
foreground start and historical status. A relocated binary completes protected
terminal setup/enrollment and mixed verified-TLS delivery/restart using controlled
fixtures. The [getting-started guide](../operations/getting-started.md) describes
source installation and operation. These results establish local implementation,
not real-cluster/provider permission or a supported release.

| Area | Remaining evidence or decision |
| --- | --- |
| Cluster/site | Explicit authorization, polling policy, Slurm/version/accounting/job-identity/timezone baseline and real-source acceptance |
| Real providers | Authorized intended endpoints/accounts, trust/auth/refusal/unknown-acceptance and recovery review |
| Platform/install/upgrade | Intended target/filesystem operator acceptance, MSRV/libc matrix, closed backup/migration/rollback and exact release-artifact installation |
| Security/governance | Maintainer threat/dependency-license review, private reporting channel availability and repository review/check protection |
| Release | Explicit approval, binaries, checksums/provenance/notices and upgrade notes; none published |

The [security policy](../../SECURITY.md) provides reporting guidance without promising
an SLA. The [support checklist](../operations/support-and-acceptance.md) details
remaining external acceptance. Generic HTTPS and required STARTTLS SMTP are the
original local MVP scope. Explicit Slack incoming webhook rendering extends it;
installed synthetic HPC Slack delivery is operator-confirmed; real monitoring
remains pending. Teams-specific adapters, log monitoring, remote SSH and
background services are later work.

Environment value caps do not bound initial OS allocation. Effective-user NSS and
filesystem startup remain synchronous. Delivery is at least once and can duplicate
a provider-accepted request whose local completion was lost. Historical status
requires the foreground owner to stop; live queue inspection is not implemented.

The operator confirmed GPFS configuration creation/validation, installed synthetic
Slack delivery, healthy monitoring and completed array-task Slack notifications
after matching Slurm ClusterName. Local startup preflight now catches mismatches
when sibling scontrol is available, with accounting validation as fallback.
Detailed Slack v2 has controlled acceptance; its next HPC test remains pending.
Old v1 rendering and generic webhook/SMTP contracts remain intact. New v6 state
requires a closed backup before opting in; old binaries cannot read it. Names are
optional and remain in immutable protected frames for the lifetime of the ledger.
