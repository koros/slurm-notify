# Local configuration limits v1 proposal

Status: draft for review, not enforced by the executable and not an approved
configuration schema. Values below are proposed resource ceilings, not defaults,
cluster polling recommendations or supported platform guarantees. Required values
remain explicit in the configuration. Existing larger private fixture inputs do
not establish public compatibility with those inputs.

This completes the limit portion of the [contract review package](local-contract-review-v1.md).
Review these choices together with the base configuration and SMTP extension.
The [machine-readable range manifest](examples/config-limits-v1.json) is a review
artifact, not a JSON Schema or validator; it does not describe every field/type.

## Parse and structural budgets

| Subject | Proposed bound | Reason / failure behavior |
| --- | --- | --- |
| Config UTF-8 bytes | 1..65,536 | Read at most limit + 1 bytes before parsing; reject oversize, invalid UTF-8 and a byte-order mark |
| Container nesting | At most 8 | Root object is depth 1; each nested object/array increments depth; reject before unbounded parser work |
| Members per object | At most 128 | Bound validation work; decoded duplicate keys are errors even if escaped differently |
| Elements per array | At most 128 | Field-specific smaller limits also apply; no truncation |
| Cluster/provider/namespace aliases | 1..64 ASCII bytes | Letters, digits, `_` and `-`; exact, case-sensitive matching |
| Environment-variable references | 1..128 ASCII bytes | `[A-Za-z_][A-Za-z0-9_]*`; missing referenced values are errors |
| Absolute path fields | 1..4096 UTF-8 bytes | Reject NUL/control characters; path length validity still depends on the OS |
| Providers | 1..64 | Bound fan-out and align with the private mixed-route ceiling |
| Routes | 0..128 | Empty means no delivery; setup must display this explicitly |
| Events in one route | 1..9 distinct names | Current proposed lifecycle mapping only; version review governs additions |
| Destinations in one route | 1..64 distinct configured aliases | Additive route union queues at most one row per event/destination |
| SMTP permissions | 0..64 distinct exact pairs | Empty denies all; required when any SMTP provider is configured |

Reject unknown fields at every configuration object, unsupported versions, duplicate
keys, wrong scalar types, JSON numeric fractions/exponents for integer fields,
integer overflow and trailing non-whitespace bytes. Boolean is not an integer.
Require decimal integer tokens for numeric fields; no negative zero or coerced strings.
Only `delivery.jitter_seed` uses a canonical decimal string fitting `u64`.
Object-key limits count decoded keys before rejecting duplicates/unknown keys.

## Operational integer budgets

| Proposed field | Inclusive range / relationship |
| --- | --- |
| `monitor.poll_seconds` | 1..86,400; site-approved value required before monitoring |
| `monitor.query_timeout_seconds` | 1..300 and strictly less than `poll_seconds`; total active/accounting budget |
| `monitor.accounting_lookback_seconds` | 2..604,800 (seven days) |
| `monitor.accounting_overlap_seconds` | 1..86,400 and strictly less than lookback |
| `monitor.command_output_limit_bytes` | 1024..1,048,576 per stdout/stderr stream |
| `monitor.retained_identity_limit` | 1..100,000 |
| `monitor.retained_event_limit` | 1..200,000; separate from identity capacity |
| `monitor.stale_after_seconds` | 1..86,400 and at least `poll_seconds` |
| Webhook `request_timeout_seconds` | 1..30; absolute parent deadline |
| Webhook `payload_limit_bytes` | 1024..16,384; reject oversize serialization atomically |
| SMTP `request_timeout_milliseconds` | 1..30,000, whole milliseconds |
| `delivery.max_attempts` | 1..20, including first send |
| `delivery.initial_backoff_seconds` | 1..3600 |
| `delivery.max_backoff_seconds` | 1..86,400 and at least initial backoff |
| `delivery.jitter_seconds` | 0..3600 and no greater than max backoff |
| `delivery.batch_limit` | 1..256; serialized due records per tick |
| `delivery.prepared_smtp_message_limit` | 1..256; required when SMTP is configured |
| `maintenance.completed_delivery_retention_seconds` | 1..31,536,000 (365 days) |
| `maintenance.cleanup_batch_limit` | 1..256 |
| `maintenance.health_sample_limit` | 1..256; sampling is not a full-store inspection guarantee |
| `maintenance.smtp_revision_cleanup_limit` | 0..256; 0 explicitly disables rendering/provider revision pruning |

The stale, preparation and maintenance keys above extend the base draft; they are
not implemented product fields. Add them to the eventual schema only after review.
The mixed example includes explicit illustrative values for review.

Check all duration/deadline conversions, fan-out multiplication and retry due-time
arithmetic independently of these ceilings. The maximum backoff caps exponential
backoff only: a validated provider Retry-After remains a floor and can be longer.
Do not silently shorten that floor to fit a configuration limit. Failure to
represent a due time is visible and preserves pending state.

SMTP preparation is bounded before allocating messages: new selected event/SMTP
pairs must fit the configured preparation count. Each DATA payload is at most
16,384 bytes, giving a maximum 4 MiB of DATA at the proposed count ceiling, excluding
envelope/descriptor and reconciliation overhead. Oversize preparation rolls back
the poll; it does not discard events or advance the checkpoint. Repeated pressure
needs a visible diagnostic and explicit window/capacity adjustment, not silent loss.

Retained capacities protect deduplication history and pending work. Reaching a limit
fails visibly; no automatic removal of protected event/identity records is allowed.
Completed-delivery retention removes eligible delivery artifacts only, preserving
the ledger according to its separate policy. Revision pruning removes only
unreferenced descriptors, after retention, in bounded transactions. These limits
do not promise a maximum physical SQLite/WAL size or process RSS. Disk budgeting,
compaction and large-history paging need separate implementation acceptance.

## Referenced-resource budgets

Validate resources without transmitting data. Resolved HTTPS URLs are at most
4096 UTF-8 bytes; Authorization values at most 4096 ASCII bytes with no controls.
Apply URL parsing, HTTPS/userinfo/fragment/redirect restrictions from the base draft.
Do not log a rejected value or inherit proxies/custom trust without review.

SMTP DER roots retain the private bounds of 1..32 roots and 65,536 aggregate DER
bytes, with duplicate rejection and fingerprints computed from parsed content.
AUTH identity/password bounds remain 64/128 ASCII bytes. Addresses remain bounded
to 254 ASCII bytes and DATA to 16,384 bytes. Certificate validity/name verification
still runs at submission; successful config validation does not guarantee a relay
will accept authentication or mail. Do not widen private descriptor/frame bounds
merely to satisfy a proposed schema.

Resolved local file reads must be bounded and reject unexpected types/symlinks;
config byte budgets cannot constrain an unbounded environment or file resource by
themselves. Missing or oversized resources block before scheduler or network work.

## Boundary acceptance to implement after review

For every range, test minimum/maximum and one value outside each boundary. Test
fraction/exponent/overflow/boolean values, duplicate escaped keys, unknown nested
keys, unsupported versions, invalid UTF-8, excessive depth/members/elements and
trailing bytes. Test cross-field violations and maximum fan-out before allocation.
Verify failed validation performs no state creation, scheduler query or sending.

Test resource oversize without diagnostic disclosure, Retry-After exceeding the
exponential cap, retained capacity pressure, preparation rollback, completed-only
retention and protected revision bindings after reopen. Inspect installed binary
behavior separately; a JSON example syntax check is not a schema or runtime test.

Job-ID width, source timezone and occurrence-time truth remain open pending the
Slurm support baseline. This proposal does not set those limits or invent metadata.
