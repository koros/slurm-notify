# Local MVP contract review package

Status: proposed, awaiting explicit review. These documents are review artifacts;
they are not approved contracts and do not alter installed behavior. The next
implementation must preserve this distinction until review is complete.

## Documents to review together

- [Local CLI](local-cli-v1.md): concrete grammar, side effects, exit policy and output acceptance.
- [Local webhook configuration](local-webhook-config-v1.md): JSON format, explicit values, local scope and pending-route handling.
- [Local SMTP extension](local-smtp-config-v1.md): bound profiles, credentials, trust and exact pair permissions.
- [Webhook event payload](webhook-payload-v1.md): minimal metadata, opaque identity, frozen bytes and version semantics.
- [Product readiness](../architecture/product-readiness.md): implemented evidence and remaining product gates.

## Recommended decisions, still unapproved

| Decision | Recommendation | Alternative / consequence |
| --- | --- | --- |
| Config format | JSON first, strict duplicate/unknown-key rejection | YAML also requires parser maintenance/security assessment and equivalent fixtures |
| Values/defaults | Explicit operational values written by setup; no hidden polling/retention/disclosure defaults | Defaults need a reviewed support/site rationale and compatibility snapshots |
| Initial observation | Require `suppress` or `emit`; setup recommends suppress and explains it | Emitting discovered history increases catch-up notifications; selected policy remains durable |
| CLI activation | Foreground-only local mode with explicit config path and plain output | Daemons, implicit paths and JSON output expand the contract and acceptance scope |
| First activated slice | Local webhook runtime, followed by SMTP before declaring the public MVP complete | Activate both at once with a larger resource-loading/onboarding test surface |
| Payload privacy/identity | Minimal structured fields, persisted opaque IDs and immutable bytes | Exporting internal IDs or additional job metadata needs a privacy proposal |
| SMTP | Verified required STARTTLS, explicit trust/auth identity and enrolled exact profile pairs | Implicit TLS, system/PEM trust and other auth modes need additional bounded transport/resource acceptance |
| Reconfiguration | Restart-based changes; block pending work lacking its original approved resources | Retargeting/discard requires explicit, separately reviewed reconciliation behavior |
| Public library API | None | Any supported Rust API requires its own contract baseline |

SMTP remains an MVP requirement. The first activated slice is an implementation
sequence, not a scope waiver. No real cluster, provider account, service installer
or release is authorized by approving these drafts.

## Limits that must be settled before a schema is frozen

The existing config draft intentionally does not prescribe product-wide upper
bounds. Review maximum config bytes/depth, provider/route counts, scheduler stream
bytes, retained identity/event counts, history/poll durations, payload bytes, retry
attempts/delays and maintenance batch sizes. Safe integer conversion/allocation
and deadline arithmetic are mandatory regardless of chosen bounds.

Current private SMTP evidence supports 64 permission entries, descriptors up to
4096 bytes, at most 32 trust roots/65,536 DER bytes, DATA up to 16,384 bytes and
1..30,000 ms supervised requests. These are implementation bounds, not approval of
all product resource limits. Public job-ID widths and source timestamp/timezone
semantics depend on the Slurm support baseline. Do not fabricate occurrence times.

## Compatibility and validation package after review

1. Record accepted decisions and revisions, including resource limits and open items.
2. Add reviewed CLI snapshots and config/event schemas under `contracts/`, with
   representative valid/invalid/version fixtures. Compare additions, removals,
   type/meaning/default/privacy changes; never regenerate baselines to pass CI.
3. Assess parser, UUID source and any new dependencies before adopting them. Persist
   opaque event IDs and frozen webhook bytes atomically before public sending.
4. Implement the smallest reviewed runtime slice privately, then activate installed
   commands with fresh-install, no-hidden-side-effect and restart acceptance.
5. Keep unresolved support, cluster policy, licensing, security and release gates
   visible. Pinning the development toolchain does not set the product support matrix.

Approval may accept individual documents with edits; it must identify the agreed
semantics. A generic request to continue implementing the prototype is not treated
as approval of newly proposed public surfaces. No approved snapshots are written by
this package, and compatibility guarantees begin only with reviewed baselines.
