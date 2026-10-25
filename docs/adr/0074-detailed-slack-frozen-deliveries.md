# ADR 0074: optional detailed Slack deliveries

Status: adopted, 2026-10-25.

The operator confirmed installed Slack tests and completion notifications for real
array tasks. Add useful accounting details without changing the generic webhook
payload, SMTP rendering, event identity or existing Slack v1 revision.

Slack providers accept optional boolean `include_job_details` and `include_job_name`,
both false when absent. Either true selects the v2 renderer; job-name inclusion
also enables the detail collection. New examples enable details but hide names.
Initialization previews the new fields, name disclosure and state-lifetime retention.
No credentials or raw names appear in operational logs. Names are private opt-in
metadata and never enter generic event frames, SMTP messages or other Slack routes
that hide names. No new dependencies, polling defaults or automatic catch-up alerts.

When a detailed route exists, extend the existing bounded current-user accounting
query with `ElapsedRaw,End`, and append `JobName%256` only if names are requested.
Keep the usual local/array/allocation/duplicate/time-window filters and shared query
budget. The last-field name can contain pipe delimiters; malformed multi-line
records still reject the snapshot. Accept old seven-field fixtures/records as
unknown optional detail values. Optional accounting fields are interpreted only
with pinned `TZ=UTC`, C locale and cleared formatting overrides. Validate timestamps
before appending Z; unknown values remain unavailable. Pending Start estimates are
not start times; active-only records do not invent exit status. ExitCode is the
allocation script's status:signal pair, not DerivedExitCode or a promised root cause.

Record metadata transiently alongside the candidate lifecycle snapshot, selecting
metadata consistent with the reconciled state. Freeze the rendered Slack JSON per
event/destination inside the same transaction as the canonical UUID/frame, routes,
checkpoint and outbox. Generic frame bytes stay unchanged. The destination revision
binds Slack v2 plus the name-disclosure choice; pending work cannot change renderers
or disclosure. Missing/corrupt/changed bindings fail closed. Retrying uses the stored
bytes, without another accounting query to refresh their details.

Migration 0006 runs only inside the first selected detailed Slack event transaction,
after migration 0005 if needed. It creates immutable `slack_event_frames` with
route foreign keys, byte bounds and SHA-256 integrity. Failure rolls back the schema
and entire poll. Existing v4/v5 state remains readable; new binaries also accept v6.
Old binaries reject v6, so stop monitoring, drain pending work and take a closed
state/config backup before enabling details. Downgrade requires restoring that
backup and can replay alerts accepted after it; never lower user_version by hand.

As with existing immutable public event frames, detailed Slack frames remain for
the lifetime of the retained ledger. Outbox retention is not metadata erasure.
Opted-in names therefore persist in protected state/backups until that state is
retired. This is disclosed in init and the upgrade guide. No automatic history reset.

Render plain text with status icons (✅ completed, ❌ failed, ⏱️ timeout and
distinct other states), separate allocation/array-task labels, optional name, elapsed
duration, status:signal, start/end UTC values, observation time and final event ID.
Unknowns are explicit. Escape Slack special name characters and disable Markdown
and unfurls. Keep the 4000-byte text and configured canonical/wire payload caps;
never truncate identifiers or invent zero statistics. Synthetic tests remain free
of real scheduler data. Partition/memory statistics, array summaries and rich
Block Kit footers remain future extensions.

Controlled acceptance covers bounded collection, privacy, unknown values, rollback,
changed-disclosure guards, restart and byte-identical HTTPS retries. Detailed real
HPC notifications still need operator acceptance.
