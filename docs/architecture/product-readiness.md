# Product readiness assessment

Assessment after selected mixed-route runtime-cycle acceptance and private bounded
Rust JSON decoding, draft-shape validation and private webhook/mixed composition.
This supersedes the initial scaffold assessment for current readiness; historical
entries in the implementation plan describe individual increments.

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
- Shared local checks, staged/pre-push hooks and a Rust coverage gate of at least 80%.

These results use synthetic scheduler data, temporary owned state and controlled
process/loopback fixtures. They establish tested prototype behavior, not supported
cluster compatibility or a working installed notification agent. Implicit TLS in
the bound supervised SMTP path remains blocked.

## Evidence and limits

The latest implementation milestone passed 339 Rust tests plus one ignored helper,
custom process acceptance, eight SQLite tests, six hook tests and 11 draft-config
Python tests. Rust line coverage was 97.53% (19,902/20,406); the gate is 80%.
These are recorded local results, not a claim that hosted checks currently pass.

| Private evidence | Location | Remaining integration gap |
| --- | --- | --- |
| Protected configuration and DER trust reads | [Config snapshot](../../crates/notify-cli/src/config_snapshot.rs), [trust snapshot](../../crates/notify-cli/src/trust_snapshot.rs) | Private bounded Rust decoding and draft-shape/control validation exist; Webhook and mixed SMTP injected composition exist; Composed runtime acceptance supports per-destination SMTP deadlines; Protected snapshot-to-runtime acceptance exists; Composed guarded state acquisition exists for the fixed private basename; independent configured capacity budgets and installed parsing remain pending |
| Owned secret/provider resources | [SMTP resolution](../../crates/notify-cli/src/provider_resolution.rs), [webhook resolution](../../crates/notify-cli/src/webhook_resolution.rs), [process acceptance](../../tests/integration/transport_boundary.rs) | Lookups/helpers remain injected; installed explicit environment resolution must enforce bounds and redaction |
| Operational and observation policy mapping | [Controls](../../crates/notify-cli/src/runtime_mapping.rs), [identity mapping](../../crates/notify-cli/src/observation_mapping.rs) | No effective-user OS adapter or installed configuration mapping |
| Selected mixed poll/dispatch/reopen | [Route selection](../../crates/notify-cli/src/route_selection.rs), [runtime cycles](../../crates/notify-cli/src/polling_cycle.rs) | Caller-driven ticks; no foreground loop, signal handling or blocked-batch scheduling acceptance |
| Webhook wire behavior | [Webhook transport](../../crates/notify-cli/src/webhook.rs), [payload proposal](../proposals/webhook-payload-v1.md) | Public opaque IDs, timestamps and frozen JSON payload bytes are not implemented |

In particular, frozen SMTP messages do not establish frozen public webhook payloads.
Controlled SMTP result helpers do not establish real provider acceptance. The
private configuration validator is Python review tooling, not an installed parser.

## Work before a usable MVP

| Area | Current gap | Required evidence or decision |
| --- | --- | --- |
| Public contracts | CLI/config/event examples remain proposals; contract baselines are unapproved | Concrete command, output, exit-code, schema/default and migration review, then approved compatibility fixtures |
| Installed runtime | Implementation modules are test-only; executable exits with unavailable status | Activate reviewed contracts, bounded foreground runtime, owned state and supervised child dispatch; fresh-install acceptance |
| Resource resolution | Protected reads and owned injected resources are tested, but installed resource mapping is absent | Reviewed parser/config-to-profile mapping, explicit bounded lookup and installed permission/error acceptance |
| Onboarding | No usable setup wizard, test, doctor or status commands | Reviewed flows, safe opt-in test delivery, accessible/noninteractive behavior and newcomer acceptance |
| Scheduler policy | No supported Slurm/site baseline or real-cluster acceptance | Validate command features, accounting lag, job identity and permitted polling/execution at an authorized cluster |
| Provider scope | Generic webhook/SMTP prototypes exist; Slack/Teams priority remains open | Agree MVP scope, payload compatibility and supported provider setup; test real providers only when authorized |
| Platform support | Development toolchain is pinned; product MSRV/libc/Slurm matrix remains undecided | Target builds, installation and process/permission/deadline smoke tests; mark unverified targets explicitly |
| Governance/security | License decision remains in LICENSE.pending.md; release/security governance is incomplete | Choose license, reporting/support policy and required review/check protection; complete threat/dependency review |
| Packaging/releases | No published agent binaries or user installation acceptance | Human-approved release plan, checksums/provenance, target smoke tests, upgrade documentation and release gates |

Log monitoring and remote SSH are later design phases. Their discovery, privacy,
rotation, host-key, reconnect and platform acceptance remain outstanding.

## Next sequence

1. Review the prepared [local contract package](../proposals/local-contract-review-v1.md)
   and settle resource limits, initial-scan/catch-up semantics and pending-profile UX.
   The proposal does not establish a supported contract.
2. After explicit contract review, activate the smallest foreground local runtime
   slice using the verified internal boundaries and controlled install fixtures.
3. Complete public event persistence/serialization, effective-user resolution,
   configuration-to-resource composition and bounded blocked-work scheduling before
   claiming monitoring. Add onboarding/diagnostics in separate increments.
4. Validate the agreed support matrix and cluster/provider policy, then complete
   packaging and release acceptance. Do not publish a release without authorization.

No percentage-complete estimate is assigned: product decisions and external
acceptance gates are substantial work even though the private pipeline is tested.

## Proposed activation milestones after review

1. Establish reviewed CLI/config/event baselines and valid/invalid/version fixtures.
   Review parser and opaque-ID dependencies; do not treat draft Python checks as a
   production parser. Exact CLI text and unresolved source identity/time bounds need
   review before activation.
2. Implement protected Rust configuration decoding and explicit identity/resource
   composition. Verify invalid inputs fail before querying or sending and do not
   expose private resource values.
3. Persist opaque public event IDs and frozen webhook JSON in the poll transaction.
   Verify rollback, collisions, restart, suppression and edited destinations; retain
   documented replay risk after remote acceptance and failed local commit.
4. Activate reviewed finite commands and then the bounded foreground webhook loop.
   Installed-binary acceptance must cover signals, blocked batches without spinning,
   ownership loss, partial progress and restart. SMTP activation follows with exact
   profile permissions and resource recovery; both are required for the public MVP.
5. Complete setup/diagnostics, permitted cluster/provider acceptance and installation
   gates. Licensing/support decisions and release authorization remain separate.

This sequence is a review plan; no public contract or activation is approved here.
