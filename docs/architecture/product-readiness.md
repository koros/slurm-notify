# Product readiness assessment

Assessment after installed configuration validation and historical status inspection.
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
process/loopback fixtures. They establish tested prototype behavior, not supported
cluster compatibility or a working installed notification agent. Implicit TLS in
the bound supervised SMTP path remains blocked.

## Evidence and limits

The latest implementation milestone passed 407 private Rust tests plus nine installed CLI acceptance tests
and six provider-codec checks and four ignored child helpers,
custom process acceptance, eight SQLite tests, six hook tests and 11 draft-config
Python tests. Rust line coverage was 97.15% (25,117/25,854); the gate is 90%.
These are recorded local results, not a claim that hosted checks currently pass.

| Private capability | Evidence | Remaining installed gap |
| --- | --- | --- |
| Protected config and trust | [Config snapshot](../../crates/notify-cli/src/config_snapshot.rs), [Rust decoder](../../crates/notify-cli/src/config_decode.rs), [draft validator](../../crates/notify-cli/src/config_validate.rs), [trust snapshot](../../crates/notify-cli/src/trust_snapshot.rs) | Installed protected webhook/SMTP validation, schema and historical status exist; monitoring bindings remain pending |
| Identity, controls and resources | [Identity mapping](../../crates/notify-cli/src/observation_mapping.rs), [controls](../../crates/notify-cli/src/runtime_mapping.rs), [SMTP resolution](../../crates/notify-cli/src/provider_resolution.rs), [webhook resolution](../../crates/notify-cli/src/webhook_resolution.rs) | Private environment and effective-UID adapters exist; installed bindings and bounded account-lookup duration remain pending |
| Owned state and status | [Session](../../crates/notify-cli/src/foreground_session.rs), [status composition](../../crates/notify-cli/src/status_composition.rs), [status process acceptance](../../tests/integration/status_boundary.rs) | Installed historical status/output snapshots exist; inspection requires existing closed rollback-journal state and cooperating exclusive ownership |
| Mixed routing and durable recovery | [Runtime cycles](../../crates/notify-cli/src/polling_cycle.rs), [route selection](../../crates/notify-cli/src/route_selection.rs), [active process acceptance](../../tests/integration/active_tick_boundary.rs) | Installed resource and exact SMTP permission enrollment; transactional public frames and mixed SMTP preparation/admission are tested; mixed public delivery composition is tested; public runtime-cycle integration is tested; owned-session public ticks and continuous startup/shutdown orchestration are tested; installed monitoring remains pending |
| Foreground orchestration | [Unified runtime](../../crates/notify-cli/src/foreground_runtime.rs), [schedule](../../crates/notify-cli/src/foreground_schedule.rs), [shared wait](../../crates/notify-cli/src/foreground_wait.rs), [process acceptance](../../tests/integration/foreground_boundary.rs) | Combined OS lifecycle and cooperative callback recovery are tested; first-signal retention and continuous scheduler exist; owned continuous poll-budget renewal is tested; installed exit mapping and bindings remain pending |
| Bounded lifecycle diagnostics | [Reports](../../crates/notify-cli/src/foreground_report.rs), [startup process acceptance](../../tests/integration/startup_boundary.rs) | Reviewed public output and exit policy; private summaries are static ASCII capped at 96 bytes |
| Provider transport | [Webhook](../../crates/notify-cli/src/webhook.rs), [transport process acceptance](../../tests/integration/transport_boundary.rs) | Public UUID/frozen JSON/endpoint revisions are tested internally; stored-frame supervised adapter has controlled child acceptance; mixed delivery composition has partial-completion/restart acceptance; public runtime-cycle integration is tested; owned-session public ticks and continuous orchestration are tested; installed monitoring and authorized real-provider acceptance remain pending |

Private Rust decoding and validation are implemented. Python draft checks provide
additional review fixtures; installed config schemas and validation are now recorded
under contracts/config.
Frozen public webhook payloads have transaction/identity/revision tests. Mixed
public/SMTP preparation and admission are tested; stored-frame supervised dispatch is tested with controlled children. Mixed delivery composition preserves SMTP permission gating and partial completion.
Public runtime-cycle integration is tested; owned-session public ticks and continuous startup/shutdown orchestration are tested; installed monitoring remains pending. Controlled SMTP helpers do not establish real-provider acceptance. Cancellation is cooperative
at hook/stage boundaries; synchronous lookup/filesystem hooks do not acquire a
universal deadline from the foreground scheduler.

## Work before a usable MVP

| Area | Current gap | Required evidence or decision |
| --- | --- | --- |
| Public contracts | Help/version, config validation and status have baselines; broader MVP interfaces adopted; public event persistence and monitoring acceptance pending | Concrete command, output, exit-code, schema/default and migration review, then approved compatibility fixtures |
| Installed runtime | Help/version, config validation and historical status are installed; monitoring engine remains test-only | Activate reviewed contracts, bounded foreground runtime, owned state and supervised child dispatch; fresh-install acceptance |
| Resource resolution | Installed validation checks webhook/SMTP resources and exact permissions; monitoring bindings remain pending | Reviewed parser/config-to-profile mapping, explicit bounded lookup and installed permission/error acceptance |
| Onboarding | Historical status exists; setup wizard, provider test and doctor are pending | Reviewed flows, safe opt-in test delivery, accessible/noninteractive behavior and newcomer acceptance |
| Scheduler policy | No supported Slurm/site baseline or real-cluster acceptance | Validate command features, accounting lag, job identity and permitted polling/execution at an authorized cluster |
| Provider scope | Generic webhook/SMTP prototypes exist; Slack/Teams priority remains open | Agree MVP scope, payload compatibility and supported provider setup; test real providers only when authorized |
| Platform support | Development toolchain is pinned; product MSRV/libc/Slurm matrix remains undecided | Target builds, installation and process/permission/deadline smoke tests; mark unverified targets explicitly |
| Governance/security | MIT license selected; release/security governance is incomplete | Complete reporting/support policy and required review/check protection; complete threat/dependency review |
| Packaging/releases | No published agent binaries or user installation acceptance | Human-approved release plan, checksums/provenance, target smoke tests, upgrade documentation and release gates |

Log monitoring and remote SSH are later design phases. Their discovery, privacy,
rotation, host-key, reconnect and platform acceptance remain outstanding.

## Next sequence and activation decisions

The [contract review package](../proposals/local-contract-review-v1.md) contains
concrete CLI, config, SMTP, limits and payload proposals. Its local MVP recommendations were adopted on 2026-10-13. Before further installed
activation, record schemas/snapshots and implement acceptance for:

- Strict JSON and explicit operational values, including limits, initial observation
  and catch-up policy.
- Foreground command grammar, exact output/exit policy and restart-based resource
  changes, including how users diagnose blocked pending work.
- Minimal payload fields, opaque identity, timestamp/null semantics and compatibility;
  source identity widths/timezones need a supported Slurm baseline.
- Required STARTTLS SMTP, explicit trust/auth identity and exact profile permissions;
  webhook-first activation is sequencing, with SMTP still required for the MVP.

The following implementation sequence reuses the completed private pipeline:

1. Record reviewed contracts and positive/negative/version compatibility fixtures.
   UUID generation now reuses ring SystemRandom; maintain deliberate baselines.
2. Help/version activation is complete. Bind later reviewed
   finite commands to existing private protected preparation, OS resource adapters
   and status inspection; verify installed failures, privacy and side effects.
3. Webhook public-event transactions now freeze UUIDs, bytes and endpoint revisions
   atomically with tested collision/rollback/suppression/restart behavior. Complete
   mixed SMTP composition and supervised dispatch; unknown source times stay null.
4. Activate reviewed finite commands and foreground orchestration. Fresh installed
   binary acceptance must cover invalid inputs before access, ownership, signals,
   blocked scheduling without spinning, partial delivery and resource recovery.
   Reuse protected Rust preparation, mixed composition and scheduling already tested.
5. Complete setup/test/doctor UX and authorized cluster/provider acceptance. Settle
   support baselines and dependency notices, then installation/upgrade and release gates.

The [private environment adapter](../../crates/notify-cli/src/environment_resolution.rs)
composes with the resource boundary; its accepted-value cap does not bound the
initial OS allocation. The [effective-user adapter](../../crates/notify-cli/src/effective_user.rs)
has bounded scratch storage but synchronous NSS duration. Combined signal/resource
lifecycle and cooperative active-callback recovery are tested privately.

The [updated activation review](../proposals/local-contract-review-v1.md) identifies
remaining installed 130-vs-143 mapping, OS-backed continuous bindings, webhook-only
composition, live-status access and mixed public-event/SMTP composition gaps. The
[help/version slice](../proposals/finite-cli-activation-v1.md) is approved and
implemented; further installed commands and public schema choices remain pending.

No percentage-complete estimate is assigned: unresolved product decisions and
external acceptance are substantial work. This sequence approves no public contract,
real-cluster test, external notification or release.
