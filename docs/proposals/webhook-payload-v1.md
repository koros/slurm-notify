# Webhook payload v1 proposal

Status: draft for review. These fields and examples are not supported interfaces,
approved contract baselines, or outputs of the current executable. No serializer,
public schema, database migration, or runtime behavior is introduced here.

## Proposed scope

A generic HTTPS destination receives one UTF-8 JSON lifecycle event per POST.
The initial payload contains scheduler identity and state only. Exclude usernames,
job names, command lines, working directories, paths, environment, credentials,
logs, free-form scheduler output, and arbitrary details. Additional metadata would
require an explicit privacy/configuration proposal. A configured cluster alias is
still identifying information; the setup flow must show exactly what is sent.

See [synthetic completed-array example](examples/webhook-completed-array.json).

| Field | Proposed representation and meaning |
| --- | --- |
| schema_version | Required string `1.0`; payload major/minor version, independent of package/config versions. |
| event_id | Required `evt_` plus 32 lowercase hex digits representing a persisted UUID v4. Opaque, stable across destination delivery retries and restarts. |
| source | Required constant `slurm`. |
| cluster_id | Required user-selected alias, 1–64 ASCII letters/digits/underscore/hyphen; never inferred from an SSH host or private hostname. |
| event_type | Required lifecycle name from the mapping below. |
| severity | Required notification classification from the mapping below; does not assert application log severity. |
| observed_at | Required UTC timestamp `YYYY-MM-DDTHH:MM:SSZ`, from first committed observation of this event, frozen across retries. |
| occurred_at | Required UTC timestamp or JSON null; scheduler event time only when independently available and its timezone is known. Never substitute observation/delivery time. |
| job.id | Required canonical decimal string for the allocation ID; preserve precision and reject signs, spaces, leading zeros. |
| job.array_task_id | Required canonical non-negative decimal string or null for a non-array allocation; zero is allowed. |
| job.submitted_at | Required UTC timestamp or null if source timezone cannot be established. |
| job.started_at | Required actual UTC start timestamp or null for a pre-start cancellation/unknown source timezone. Never transmit a pending start estimate as actual. |
| job.state | Required canonical Slurm state associated with this event, frozen even if a later state is observed. |

Do not include ExitCode in v1 until the ledger retains the source value consistently.
A `COMPLETED` event describes scheduler state, not proof that the research result is
correct. Future exit-code fields must distinguish scheduler exit code, signal, and
DerivedExitCode. No summary string is required; receivers can format structured data.
Canonical decimal strings use `0` or a nonzero digit followed by digits; job.id
must be positive. Public limits for large numeric identifiers require Slurm support
baseline validation rather than guessing an integer width.

## Lifecycle mapping

| Slurm event state | event_type | severity |
| --- | --- | --- |
| RUNNING | JOB_STARTED | INFO |
| COMPLETED | JOB_COMPLETED | INFO |
| FAILED | JOB_FAILED | ERROR |
| TIMEOUT | JOB_TIMEOUT | ERROR |
| CANCELLED | JOB_CANCELLED | WARN |
| OUT_OF_MEMORY | JOB_OUT_OF_MEMORY | ERROR |
| NODE_FAIL | JOB_NODE_FAILURE | ERROR |
| BOOT_FAIL | JOB_BOOT_FAILURE | ERROR |
| DEADLINE | JOB_DEADLINE | ERROR |

PENDING, unknown states, PREEMPTED, and ambiguous attempt identity produce no public
lifecycle event until their semantics are agreed. Job steps, compressed array ranges,
and heterogeneous allocation identities are outside this first proposal. A terminal
event wins within a known attempt; subsequent requeue attempts have distinct IDs.
Initial/catch-up observation policy controls whether an event is routed, not its
payload shape. Suppressed observations must remain suppressed after restart.

## Identity, freezing, and delivery

Keep the internal identity tuple (cluster, full allocation/task ID, submission,
actual-start context, state) for deduplication. Do not export the current private
length-prefixed ledger ID, encode it into the public ID, or assume a hash conceals
its contents. Generate an independent opaque ID once and commit its mapping with
the event and routed outbox records. Collision detection and retry belong inside
that transaction. The example ID is synthetic; UUID generation is not implemented.

Freeze the payload's bytes and public ID before delivery. All selected destinations
receive the same bytes; retrying cannot update observed_at, state, or timestamp
formatting. A later destination/config change does not rewrite an existing outbox
record's payload or route set. Deleted/replaced destinations require explicit
pending-record reconciliation rather than silently sending to a new URL with the
same name. Stored payloads contain user data and require secure state permissions.

Propose `Content-Type: application/json` and `Idempotency-Key: <event_id>`.
Receivers may ignore the key; acceptance followed by a failed local commit can
replay the exact event. 2xx means accepted, not human receipt or exactly-once delivery.
The current HTTP outcomes, deadlines, Retry-After floors, completion-time scheduling,
and jitter remain prototype semantics pending operational review. Ordering across
destinations/events is not guaranteed. No secret, request URL, or recipient is part
of the JSON payload.

## Compatibility proposal and implementation gaps

Receivers should tolerate unknown additive fields within major version 1. They
should handle unknown event_type/state values as unsupported rather than mapping
them to success. Type, meaning, required-field, privacy, or default changes require
review; incompatible changes require a new major schema version. Published sender
schemas must describe additive-field handling and version negotiation explicitly.

The prototype lacks persisted opaque IDs, frozen payload bytes, occurrence-time
queries, source timezone guarantees, and safe pending-route reconfiguration. Its
Event contains only identity/state, and ExitCode is not durable event data. Public
payload implementation must address these gaps rather than fabricate metadata.
No approved contract is written under contracts/events by this draft.

Review decisions: minimal fields; opaque persisted IDs; event/severity mapping;
null/timezone policy; byte freezing and route changes; version compatibility; limits
and privacy. After review, add machine-readable schemas, representative positive
and negative fixtures, and a compatibility baseline before any public delivery.
