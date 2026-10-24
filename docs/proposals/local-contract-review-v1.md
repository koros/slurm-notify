# Local MVP contract review package

Status: local MVP contract proposals adopted by the maintainer on 2026-10-13.
Approval covers strict JSON/explicit settings, local foreground CLI, generic HTTPS
webhook plus required STARTTLS SMTP, minimal frozen events and the proposed limits.
Implementation/installed acceptance and remaining support/source-data decisions are
still required; approval does not imply those capabilities are installed.

## Documents to review together

- [First installed CLI slice](finite-cli-activation-v1.md): exact help/version grammar, outputs and independent approval scope.
- [Local CLI](local-cli-v1.md): concrete grammar, side effects, exit policy and output acceptance.
- [Local webhook configuration](local-webhook-config-v1.md): JSON format, explicit values, local scope and pending-route handling.
- [Local SMTP extension](local-smtp-config-v1.md): bound profiles, credentials, trust and exact pair permissions.
- [Webhook event payload](webhook-payload-v1.md): minimal metadata, opaque identity, frozen bytes and version semantics.
- [Configuration limits](local-config-limits-v1.md): proposed structural/resource bounds and boundary acceptance.
- [Product readiness](../architecture/product-readiness.md): implemented evidence and remaining product gates.

The separate finite help/version slice was approved on 2026-10-12 and implemented.
The broader local MVP recommendations below were adopted on 2026-10-13.

## Adopted local MVP decisions

| Decision | Recommendation | Alternative / consequence |
| --- | --- | --- |
| Config format | JSON first, strict duplicate/unknown-key rejection | YAML also requires parser maintenance/security assessment and equivalent fixtures |
| Values/defaults | Explicit operational values written by setup; no hidden polling/retention/disclosure defaults | Defaults need a reviewed support/site rationale and compatibility snapshots |
| Initial observation | Require `suppress` or `emit`; setup recommends suppress and explains it | Emitting discovered history increases catch-up notifications; selected policy remains durable |
| CLI activation | Foreground-only local mode with explicit config path and plain output | Daemons, implicit paths and JSON output expand the contract and acceptance scope |
| First activated slice | Exact help/version slice, then reviewed finite config/status commands, then webhook and SMTP monitoring | Foreground-first requires completing payload, signal-reason and continuous-scheduling gaps before installed activation |
| Payload privacy/identity | Minimal structured fields, persisted opaque IDs and immutable bytes | Exporting internal IDs or additional job metadata needs a privacy proposal |
| SMTP | Verified required STARTTLS, explicit trust/auth identity and enrolled exact profile pairs | Implicit TLS, system/PEM trust and other auth modes need additional bounded transport/resource acceptance |
| Reconfiguration | Restart-based changes; block pending work lacking its original approved resources | Retargeting/discard requires explicit, separately reviewed reconciliation behavior |
| Public library API | None | Any supported Rust API requires its own contract baseline |

SMTP remains an MVP requirement. The first activated slice is an implementation
sequence, not a scope waiver. No real cluster, provider account, service installer
or release is authorized by approving these drafts.

## Limits that must be settled before a schema is frozen

The [limits draft](local-config-limits-v1.md) now proposes concrete bounds and
cross-field validation, with a machine-readable review manifest and updated
synthetic examples. It adds explicit stale/preparation/maintenance fields to the
configuration draft. These adopted choices are ceilings, not defaults or cluster policy recommendations.
Record schema baselines and retain the existing bounded validator acceptance.

Current private SMTP evidence supports 64 permission entries, descriptors up to
4096 bytes, at most 32 trust roots/65,536 DER bytes, DATA up to 16,384 bytes and
1..30,000 ms supervised requests. The proposed product limits are now adopted; implementation acceptance remains. Public job-ID widths and source timestamp/timezone
semantics depend on the Slurm support baseline. Do not fabricate occurrence times.

## Private evidence available for reuse

Protected Rust decoding/validation, owned state/status, mixed ticks, monotonic
finite scheduling, static lifecycle reports, effective-UID/environment adapters and
SIGINT/SIGTERM binding now exist privately. Combined OS resource/signal lifecycle
and active-callback recovery are tested. See [ADR 0057](../adr/0057-private-combined-os-foreground-lifecycle.md)
and the readiness evidence. Python draft checks are supplementary review tooling.
The installed protected parser now serves webhook-only configuration validation
with structural webhook/SMTP schemas and exact output baselines. Offline historical
status is installed without provider resource resolution. Live status remains separate; public event emission now has controlled installed acceptance.

| Area | Current implementation / remaining gate |
| --- | --- |
| CLI/config/status | All local MVP commands have deliberate grammar/help/output and installed acceptance; final release compatibility review remains |
| Status while monitoring | Historical status reports busy under exclusive ownership; process health stays unknown; live inspection is separate |
| Signals and foreground | Installed SIGINT/SIGTERM exits 130/143, owned continuous orchestration and bounded active-stage recovery are accepted |
| Resource lifetime | Protected snapshots, explicit trust and permissions are installed; environment allocation/NSS/filesystem startup do not have universal hard deadlines |
| Webhook/mixed events | Frozen UUID/JSON/endpoint revisions, mixed SMTP composition and actual verified TLS dispatch/restart are accepted with controlled fixtures |
| Setup/tests/doctor | Protected interactive draft review/enrollment, one-shot synthetic tests and read-only bounded probes are installed |
| External support | Authorized cluster/provider, source identity/timezone, platform/install/upgrade and exact release-artifact acceptance remain |

These are distinct from already tested cooperative callback cancellation and frozen
SMTP recovery. Private 96-byte summaries are not public CLI snapshots. Document
review settles semantics; installed acceptance still has to prove implementation.

## Compatibility and validation package after review

1. Record accepted decisions and revisions, including resource limits and open items.
2. Add reviewed CLI snapshots and config/event schemas under `contracts/`, with
   representative valid/invalid/version fixtures. Compare additions, removals,
   type/meaning/default/privacy changes; never regenerate baselines to pass CI.
3. Reassess reuse of the private Rust decoder and dependencies for installed parsing;
   assess the UUID source and any new dependencies before adopting them. Persist
   opaque event IDs and frozen webhook bytes atomically before public sending.
4. Implement the smallest reviewed runtime slice privately, then activate installed
   commands with fresh-install, no-hidden-side-effect and restart acceptance.
5. Keep unresolved support, cluster policy, security and release gates
   visible. Pinning the development toolchain does not set the product support matrix.

Review can accept individual documents with edits and record agreed semantics. No approved snapshots are written by
this package, and compatibility guarantees begin only with reviewed baselines.

## Review completion checklist

Reviewers should record acceptance or requested edits for the CLI, base config,
SMTP extension, event payload and limits documents above. The recommendation is
JSON with explicit operational values, local foreground mode, minimal frozen
webhook payloads and required STARTTLS SMTP, with no public Rust API.

Acceptance must also identify deferred items and who will settle them. Public job
identity widths/source timestamps, exact CLI snapshots, parser/UUID dependencies,
blocked-work scheduling and pending-resource reconciliation need decisions or
reviewed acceptance before the affected behavior is activated. A document review
is not proof those implementation gates pass.

The [first installed CLI proposal](finite-cli-activation-v1.md) is intentionally
independent: approving it does not accept the broader config/event/SMTP drafts.
The adopted scope excludes unsupported source metadata, a public Rust API and
real-cluster/provider runs or release publication. The
[readiness sequence](../architecture/product-readiness.md) reuses completed private
OS composition while retaining public payload and installed acceptance gates.
