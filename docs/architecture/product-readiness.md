# Product readiness assessment

Assessment after mixed-route cancellation and resolution recovery acceptance.
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
- Shared local checks, staged/pre-push hooks and a Rust coverage gate of at least 80%.

These results use synthetic scheduler data, temporary owned state and controlled
process/loopback fixtures. They establish tested prototype behavior, not supported
cluster compatibility or a working installed notification agent. Implicit TLS in
the bound supervised SMTP path remains blocked.

## Work before a usable MVP

| Area | Current gap | Required evidence or decision |
| --- | --- | --- |
| Public contracts | CLI/config/event examples remain proposals; contract baselines are unapproved | Concrete command, output, exit-code, schema/default and migration review, then approved compatibility fixtures |
| Installed runtime | Implementation modules are test-only; executable exits with unavailable status | Activate reviewed contracts, bounded foreground runtime, owned state and supervised child dispatch; fresh-install acceptance |
| Resource resolution | Caller injects helper commands, trust and credentials | Reviewed configuration-to-profile mapping and bounded explicit secret/trust loading with permission/error tests |
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
3. Add onboarding/diagnostics and explicit resource loading in separate increments.
4. Validate the agreed support matrix and cluster/provider policy, then complete
   packaging and release acceptance. Do not publish a release without authorization.

No percentage-complete estimate is assigned: product decisions and external
acceptance gates are substantial work even though the private pipeline is tested.
