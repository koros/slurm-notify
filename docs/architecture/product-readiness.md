# Product readiness assessment

Assessment after installed verified HTTPS retry and restart acceptance.
The private OS/runtime lifecycle and continuous owned sessions are also tested.
Historical increments remain in the implementation history; this assessment groups
current evidence by capability and identifies the remaining activation gates.

## Verified private prototype

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
process/loopback fixtures. They establish tested prototype behavior, controlled installed SMTP prototype behavior, not supported
cluster compatibility or a supported monitoring release. Implicit TLS in
the bound supervised SMTP path remains blocked.

## Evidence and limits

The latest implementation milestone passed 413 private Rust tests plus fourteen installed CLI acceptance tests
and nine provider-codec checks and five ignored child helpers,
custom process acceptance, eight SQLite tests, six hook tests and 11 draft-config
Python tests. Rust line coverage was 97.14% (25,819/26,580); the gate is 90%.
These are recorded local results, not a claim that hosted checks currently pass.

| Capability | Current evidence | Remaining gap |
| --- | --- | --- |
| Protected resources | Installed validation and webhook/SMTP monitoring reuse protected configuration/trust snapshots and exact SMTP permissions | Bounded synchronous account lookup |
| State ownership | Installed status and foreground runs enforce exclusive ownership; signal shutdown releases state | Live status access and installation acceptance |
| Routing and recovery | Public event transactions, mixed admission, blocked-work preservation and restart recovery are tested | Authorized real-provider acceptance |
| Foreground runtime | Installed SMTP/mixed runs cover scheduler processes, cadence, signals, health, bounded cleanup and restart without replaying completed mail | Supported cluster and platform acceptance |
| Provider transport | Selected same-executable helpers enforce HTTPS or verified STARTTLS; controlled installed SMTP submission succeeds | Authorized real-provider acceptance |

Cancellation is cooperative at stage boundaries. Synchronous account lookup and
filesystem operations do not acquire a universal deadline from the foreground
scheduler. Controlled loopback fixtures establish no real-provider acceptance.

## Work before a usable MVP

| Area | Current gap | Required evidence or decision |
| --- | --- | --- |
| Public contracts | Help/version, config validation and status have baselines; webhook/SMTP start and public event persistence are implemented; final compatibility review remains | Concrete command, output, exit-code, schema/default and migration review, then approved compatibility fixtures |
| Installed runtime | Help/version, config validation and historical status are installed; webhook/SMTP monitoring is installed; controlled installed HTTPS retry/delivery is accepted; real-provider acceptance remains pending | Fresh-install and authorized real-provider acceptance |
| Resource resolution | Installed validation and webhook/SMTP monitoring bind protected resources and exact permissions | Document synchronous lookup limits and finish provider acceptance |
| Onboarding | Historical status exists; setup wizard, provider test and doctor are pending | Reviewed flows, safe opt-in test delivery, accessible/noninteractive behavior and newcomer acceptance |
| Scheduler policy | No supported Slurm/site baseline or real-cluster acceptance | Validate command features, accounting lag, job identity and permitted polling/execution at an authorized cluster |
| Provider scope | Generic webhook/SMTP prototypes exist; Slack/Teams priority remains open | Agree MVP scope, payload compatibility and supported provider setup; test real providers only when authorized |
| Platform support | Development toolchain is pinned; product MSRV/libc/Slurm matrix remains undecided | Target builds, installation and process/permission/deadline smoke tests; mark unverified targets explicitly |
| Governance/security | MIT license selected; release/security governance is incomplete | Complete reporting/support policy and required review/check protection; complete threat/dependency review |
| Packaging/releases | No published agent binaries or user installation acceptance | Human-approved release plan, checksums/provenance, target smoke tests, upgrade documentation and release gates |

Log monitoring and remote SSH are later design phases. Their discovery, privacy,
rotation, host-key, reconnect and platform acceptance remain outstanding.

## Next sequence and activation decisions

The [contract review package](../proposals/local-contract-review-v1.md) records local
MVP recommendations adopted on 2026-10-13. Help/version, strict configuration,
validation, historical status and SMTP/mixed foreground activation have deliberate
contract fixtures. [ADR 0065](../adr/0065-installed-smtp-mixed-foreground-command.md)
records the installed command's behavior and limits.

1. Installed HTTPS helper acceptance is complete with verified TLS, retry and durable
   restart. Broaden mixed-provider recovery evidence as onboarding is added.
2. Complete setup/test/doctor flows and review remaining output and migration
   baselines, including actionable diagnostics for blocked work.
3. Obtain authorized cluster/provider acceptance and settle platform, Slurm source
   identity/timezone and dependency support baselines.
4. Verify fresh installation and upgrade behavior, security/reporting policy and
   release gates. Publishing a release requires explicit authorization.

Environment value limits do not bound the initial OS allocation. Effective-user
lookup bounds scratch storage but retains synchronous NSS duration. Log monitoring
and remote SSH remain later phases.

No percentage-complete estimate is assigned: external acceptance and product
support decisions remain substantial work.
