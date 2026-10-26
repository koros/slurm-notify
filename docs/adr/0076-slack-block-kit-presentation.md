# ADR 0076: shared presentation content and detailed Slack Block Kit

Status: adopted for prototype implementation, 2026-10-26.

The operator approved a presentation milestone and Slack implementation first.
Use a private platform-independent Notification (heading, fields, context), with
an equivalent full plain-text fallback. Future Discord embeds and Teams Workflow
Adaptive Cards need separate provider contracts and renderers; neither is implemented.
No dependencies, scheduler queries, config fields, polling defaults or disclosure
choices change. Minimal Slack v1, generic webhook and SMTP bytes remain unchanged.

Existing detail/name opt-ins select Slack render v3, replacing v2 for new work.
Use a plain_text header with readable lifecycle titles/icons, a section of paired
plain_text fields, and context for observation time and the complete event ID.
Keep explicit unavailable details; started events omit exit status and finish time.
Names retain the separate opt-in and escaping. Synthetic tests use a distinct test
heading and no scheduler data. Top-level text includes all visible information for
accessibility and notification previews; mrkdwn and unfurls remain disabled. No
links, actions, array summaries, dynamic colour, threading or interactive controls.

Bound fallback to 4000 UTF-8 bytes, heading to 150 bytes, individual fields/context
to 2000 bytes, section fields to ten and context elements to two. Reject control
characters/empty fields and preserve configured canonical/wire budgets. The helper
accepts only the exact five-field body with canonical plain-text blocks reconstructed
from its complete fallback, or existing four-field v1/v2 bodies. Changing block
text/type, adding fields/actions or mismatching the final event ID fails closed.
The new slack-v3 schema is additive; do not alter the historical v1 schema baseline.

Bind rendering to slack-render-v3, keeping name choice and existing destination
bindings. Existing schema v6 freezes full Block Kit JSON transactionally; no database
migration is needed. Pending v2 work cannot be dispatched by v3. Before upgrading a
detailed installation, drain pending work using the previous binary/config, stop,
and take a closed state/config backup. Do not edit revisions or reset history.
Downgrade after v3 work similarly requires draining first; old binaries reject rich
frames. Historical old frozen frames remain readable by the new validator.

Controlled tests must cover all states, synthetic preview/send, hostile names,
forged blocks, bounds, name isolation, old revision guards, transactional rollback
and byte-identical HTTPS retries after restart. Real HPC layout acceptance remains
an operator gate. Existing acknowledgement/duplicate/TLS guarantees do not change.

References: [Slack Block Kit](https://docs.slack.dev/block-kit/),
[header](https://docs.slack.dev/reference/block-kit/blocks/header-block/),
[section](https://docs.slack.dev/reference/block-kit/blocks/section-block/),
[context](https://docs.slack.dev/reference/block-kit/blocks/context-block/).
