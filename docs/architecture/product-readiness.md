# Product readiness assessment

Assessment after combined private OS resources and signal-driven unified runtime
lifecycle acceptance.
The installed help/version slice is approved and tested.
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

The latest implementation milestone passed 379 private Rust tests plus four installed
CLI acceptance tests and four ignored child helpers,
custom process acceptance, eight SQLite tests, six hook tests and 11 draft-config
Python tests. Rust line coverage was 97.25% (22,188/22,815); the gate is 90%.
These are recorded local results, not a claim that hosted checks currently pass.

| Private capability | Evidence | Remaining installed gap |
| --- | --- | --- |
| Protected config and trust | [Config snapshot](../../crates/notify-cli/src/config_snapshot.rs), [Rust decoder](../../crates/notify-cli/src/config_decode.rs), [draft validator](../../crates/notify-cli/src/config_validate.rs), [trust snapshot](../../crates/notify-cli/src/trust_snapshot.rs) | Reviewed schema baselines and installed bindings; decoding and validation already exist privately |
| Identity, controls and resources | [Identity mapping](../../crates/notify-cli/src/observation_mapping.rs), [controls](../../crates/notify-cli/src/runtime_mapping.rs), [SMTP resolution](../../crates/notify-cli/src/provider_resolution.rs), [webhook resolution](../../crates/notify-cli/src/webhook_resolution.rs) | Private environment and effective-UID adapters exist; installed bindings and bounded account-lookup duration remain pending |
| Owned state and status | [Session](../../crates/notify-cli/src/foreground_session.rs), [status composition](../../crates/notify-cli/src/status_composition.rs), [status process acceptance](../../tests/integration/status_boundary.rs) | Installed command/output bindings; status inspection requires existing closed rollback-journal state and cooperating exclusive ownership |
| Mixed routing and durable recovery | [Runtime cycles](../../crates/notify-cli/src/polling_cycle.rs), [route selection](../../crates/notify-cli/src/route_selection.rs), [active process acceptance](../../tests/integration/active_tick_boundary.rs) | Installed resource and exact SMTP permission enrollment; public webhook persistence remains separate |
| Foreground orchestration | [Unified runtime](../../crates/notify-cli/src/foreground_runtime.rs), [schedule](../../crates/notify-cli/src/foreground_schedule.rs), [shared wait](../../crates/notify-cli/src/foreground_wait.rs), [process acceptance](../../tests/integration/foreground_boundary.rs) | Combined OS lifecycle and cooperative callback recovery are tested; signal-reason exit mapping, continuous scheduling and installed bindings remain pending |
| Bounded lifecycle diagnostics | [Reports](../../crates/notify-cli/src/foreground_report.rs), [startup process acceptance](../../tests/integration/startup_boundary.rs) | Reviewed public output and exit policy; private summaries are static ASCII capped at 96 bytes |
| Provider transport | [Webhook](../../crates/notify-cli/src/webhook.rs), [transport process acceptance](../../tests/integration/transport_boundary.rs) | Public opaque IDs and frozen webhook JSON; authorized real-provider acceptance |

Private Rust decoding and validation are implemented. Python draft checks provide
additional review fixtures; neither establishes an approved installed schema.
Frozen SMTP messages do not establish frozen public webhook payloads. Controlled
SMTP helpers do not establish real-provider acceptance. Cancellation is cooperative
at hook/stage boundaries; synchronous lookup/filesystem hooks do not acquire a
universal deadline from the foreground scheduler.

## Work before a usable MVP

| Area | Current gap | Required evidence or decision |
| --- | --- | --- |
| Public contracts | Help/version CLI baseline approved; broader CLI/config/event proposals remain unapproved | Concrete command, output, exit-code, schema/default and migration review, then approved compatibility fixtures |
| Installed runtime | Implementation modules are test-only; executable exits with unavailable status | Activate reviewed contracts, bounded foreground runtime, owned state and supervised child dispatch; fresh-install acceptance |
| Resource resolution | Protected reads and owned injected resources are tested, but installed resource mapping is absent | Reviewed parser/config-to-profile mapping, explicit bounded lookup and installed permission/error acceptance |
| Onboarding | No usable setup wizard, test, doctor or status commands | Reviewed flows, safe opt-in test delivery, accessible/noninteractive behavior and newcomer acceptance |
| Scheduler policy | No supported Slurm/site baseline or real-cluster acceptance | Validate command features, accounting lag, job identity and permitted polling/execution at an authorized cluster |
| Provider scope | Generic webhook/SMTP prototypes exist; Slack/Teams priority remains open | Agree MVP scope, payload compatibility and supported provider setup; test real providers only when authorized |
| Platform support | Development toolchain is pinned; product MSRV/libc/Slurm matrix remains undecided | Target builds, installation and process/permission/deadline smoke tests; mark unverified targets explicitly |
| Governance/security | MIT license selected; release/security governance is incomplete | Complete reporting/support policy and required review/check protection; complete threat/dependency review |
| Packaging/releases | No published agent binaries or user installation acceptance | Human-approved release plan, checksums/provenance, target smoke tests, upgrade documentation and release gates |

Log monitoring and remote SSH are later design phases. Their discovery, privacy,
rotation, host-key, reconnect and platform acceptance remain outstanding.

## Next sequence and activation decisions

The [contract review package](../proposals/local-contract-review-v1.md) contains
concrete CLI, config, SMTP, limits and payload proposals. Its recommendations remain
unapproved. Before installed activation, record acceptance or edits for:

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
   Assess opaque-ID dependencies; do not silently turn draft examples into baselines.
2. Help/version activation is complete. Bind later reviewed
   finite commands to existing private protected preparation, OS resource adapters
   and status inspection; verify installed failures, privacy and side effects.
3. Implement the reviewed public event transaction: opaque ID and frozen webhook
   bytes committed with routes/outbox; test collisions, rollback, suppression,
   restart and edited destinations. Establish source timestamps without fabrication.
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
remaining signal-reason/130-vs-143 mapping, finite-budget renewal, webhook-only
composition, live-status contention and public payload transaction gaps. The
[help/version slice](../proposals/finite-cli-activation-v1.md) is approved and
implemented; further installed commands and public schema choices remain pending.

No percentage-complete estimate is assigned: unresolved product decisions and
external acceptance are substantial work. This sequence approves no public contract,
real-cluster test, external notification or release.
