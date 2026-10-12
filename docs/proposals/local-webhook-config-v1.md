# Local webhook configuration v1 proposal

> Adopted for the local MVP on 2026-10-13. Statements below describing this proposal
> as unapproved are historical draft context. The [adoption record](local-contract-review-v1.md)
> defines scope; installed implementation and acceptance remain separate milestones.

Status: draft for review, not accepted configuration. The executable cannot read
this file, validate it, monitor jobs, or deliver notifications. Numeric example
values are review examples, not operational defaults or cluster recommendations.

See [synthetic explicit configuration](examples/local-webhook-config.json) and
the [proposed limits](local-config-limits-v1.md), including explicit maintenance
fields and integer cross-field rules. Neither is an approved schema.

## Format and first scope

Propose JSON for the initial local webhook configuration. This differs from the
handoff's illustrative YAML and avoids selecting a YAML parser before its maintenance
and security assessment. No JSON parser dependency is adopted in this increment.
Reject duplicate keys, unknown keys, invalid types, and invalid schema_version;
validation must complete before scheduler queries or network requests. Whether
YAML should also be accepted is an explicit review decision, not a silent change.

This proposal covers one local cluster, current-user jobs, one or more generic
webhooks, explicit routes, SQLite state, and a single serialized delivery owner.
SMTP and CLI behavior are covered by the [SMTP extension](local-smtp-config-v1.md)
and [CLI draft](local-cli-v1.md); logs, remote mode and other provider-specific
payloads remain outside this scope. Restricting this draft to webhooks does not remove SMTP from
the design handoff's first public MVP requirements.

## Proposed keys and validation

| Key/group | Proposed meaning and validation |
| --- | --- |
| schema_version | Required integer 1. Reject unsupported versions before side effects. |
| monitor.mode | Required constant `local`; no SSH fields in this scope. |
| monitor.cluster_id | Required user-selected alias using payload cluster_id rules; unique in the state namespace. |
| monitor.user_jobs_only | Required true. Resolve the effective user's scheduler identity; reject another user's filter. |
| monitor.poll_seconds | Required positive integer. No default; doctor/setup must validate against site guidance before enabling monitoring. |
| monitor.query_timeout_seconds | Required positive integer below poll_seconds; total combined active/accounting query budget. |
| monitor.accounting_lookback_seconds | Required positive integer; bounded history window, not a claim that delayed accounting is complete. |
| monitor.accounting_overlap_seconds | Required positive integer below accounting_lookback_seconds; overlap prior windows to handle late records and downtime. |
| monitor.initial_observation | Required `suppress` or `emit`; controls events discovered on first scan, including terminals. Must remain durable across restart. |
| monitor.stale_after_seconds | Required positive integer at least poll_seconds; threshold for checkpoint-age diagnostics, not proof of process liveness. |
| monitor.command_output_limit_bytes | Required positive integer per scheduler stdout/stderr stream. Reject oversized output atomically. |
| monitor.retained_identity_limit / retained_event_limit | Required positive integers. Overflow fails visibly; do not prune deduplication records and silently permit replay. |
| storage.path | Required absolute user-local path. Safe directory/file creation, symlink rejection, permissions, and locking must be implemented before use. Network filesystem semantics are not promised. |
| providers.<name>.type | Required `webhook`; destination names are unique nonempty aliases using cluster_id character/length rules. |
| providers.<name>.url_env | Required environment-variable name; resolve to HTTPS URL. Never log the value or put it in child argv. |
| providers.<name>.authorization_env | Optional environment-variable name whose entire value is the Authorization header; absent means no auth header. Missing/empty configured values are errors. |
| providers.<name>.request_timeout_seconds | Required positive integer; parent deadline covers transport/DNS and I/O, with documented cleanup latency. |
| providers.<name>.payload_limit_bytes | Required positive integer; reject serialization beyond this limit without truncating fields. |
| routes[].events | Required nonempty list of proposed lifecycle event_type names. Unknown names and repeated names within a route are errors. |
| routes[].send_to | Required nonempty list of existing destination aliases; repeated aliases within a route are errors. |
| delivery.max_attempts | Required positive integer, includes first send. Exhaustion stops that destination without fabricating an attempt. |
| delivery.initial_backoff_seconds / max_backoff_seconds | Required positive integers; maximum >= initial. The maximum caps exponential backoff only. |
| delivery.jitter_seconds | Required non-negative integer <= max_backoff_seconds; additive spreading never shortens backoff or Retry-After. |
| delivery.jitter_seed | Required non-negative integer represented as a decimal string fitting u64; avoids JSON numeric precision loss. No secrecy or cryptographic randomness claim. |
| delivery.prepared_smtp_message_limit | Required when SMTP exists; bounds newly prepared event/SMTP pairs before allocation. |
| maintenance.completed_delivery_retention_seconds | Required positive duration; completed delivery artifact retention only, not automatic ledger deletion. |
| maintenance.cleanup_batch_limit / health_sample_limit | Required positive integers; bounded maintenance and health sampling. |
| maintenance.smtp_revision_cleanup_limit | Required integer 0..256; 0 explicitly disables unreferenced rendering/provider descriptor pruning. |
| delivery.batch_limit | Required positive integer limiting due records read per batch. One serialized owner; no leases/concurrent sending claim. |

Webhook/base configuration durations are whole seconds; the SMTP extension
uses an explicitly named millisecond timeout. Base byte limits are bytes, not
Unicode character counts. Reject overflow in duration conversion, deadline
construction, allocation budgets, and retry due-time arithmetic. Product upper
bounds in the linked limits draft must be reviewed against support/resource goals
before activation; the example is not a limits schema. Initial-observation policy does not authorize an unbounded
historical replay.

## Secrets, routing, and reload

Environment names must match `[A-Za-z_][A-Za-z0-9_]*`. URLs may contain secret path
or query material, so redact the entire resolved endpoint in diagnostics. Show
scheme/host to the user during explicit setup review without printing the path,
query, or authorization. A configured destination is an explicit data-egress choice.
Public monitoring allows HTTPS only; loopback HTTP remains a test fixture facility.
Reject userinfo, fragments, header controls, and redirects. Custom headers, proxy
inheritance, custom CA stores, and certificate bypass are outside this proposal.
Enterprise proxy/CA requirements need deliberate follow-up rather than unsafe TLS.

Resolve and validate all referenced values before any sending. Children receive
secret material over bounded stdin and a cleared environment. Do not store resolved
secrets in logs, event payloads, diagnostics, argv, or an unprotected config file.
Environment-backed secrets remain visible to the owning process; keyring/file-backed
sources and rotation require future review. No inline-secret fallback is proposed.

Route matching is additive: union destinations across all matching rules, then
queue at most one event/destination record. An empty routes list is valid and sends
nothing; setup should display that clearly. No match means no delivery. Freeze the
selected route set and bytes in the same event/outbox transaction. Configuration
updates apply to new events; do not retarget existing pending events to changed
endpoints. Initially propose restart-based configuration changes, with explicit
pending-route reconciliation before a destination is removed or redefined.

## Review and remaining engineering work

No implicit defaults are proposed for required keys. An eventual wizard may write
explicit values after cluster-policy validation and review. This proposal does not
approve example poll intervals, resource caps, file paths, or first-scan behavior.

The prototype already exercises caller-driven accounting windows, secure owned
state, supervised transport and bound SMTP profiles. Installed configuration
loading, additive event-filter routing, separate retained limits, opaque public IDs,
frozen webhook payloads, webhook destination revisions, diagnostics output and
safe installed-helper/resource loading remain pending. The prototype uses a single
retained capacity for identities/events; its private SMTP tests are not approval of
this public configuration or production activation.

Review decisions: JSON versus YAML; required explicit values; first-scan semantics;
secret sources and privacy; routing/reconfiguration; resource upper bounds; HTTPS,
proxy/CA policy; state paths; version/upgrade behavior. After review, write an approved
machine-readable schema and fixtures under contracts/config, assess parser/toolchain
compatibility, and implement behind private boundaries before runtime activation.
