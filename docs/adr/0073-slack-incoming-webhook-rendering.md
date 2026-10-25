# ADR 0073: explicit Slack incoming webhook rendering

Status: adopted for prototype implementation, 2026-10-25.

The operator demonstrated an HTTP 200/`ok` Slack incoming webhook test and visible
channel message from the HPC, while Gmail SMTP connectivity timed out. This proves
that route for their Python test, not installed Rust notification acceptance.

Add explicit provider type `slack`, sharing the HTTPS webhook settings, protected
URL environment reference, optional authorization/trust resources and supervised
transport. Do not infer formatting from secret URL paths or change generic webhook
bytes. No new dependency, schema-version bump, operational default or state migration.
Existing generic webhook/SMTP configurations and revision tokens remain unchanged.

Freeze canonical public lifecycle frames as before. Render Slack text deterministically
from those frames with a `slack-render-v1` revision domain bound into the destination
revision before persistence. Renderer changes require a deliberate new revision.
Changing webhook to Slack (or back), endpoint, URL reference or trust blocks pending
work before dispatch. Routes with both kinds share the canonical UUID/frame, then
render independently. Unknown private fields are ignored; no job names, scripts,
paths, user names or logs are added. State, job/array identifier, cluster label,
observation time and public event ID are disclosed. Markdown and unfurls are disabled.
Both canonical and rendered bytes must fit the configured payload limit; text is
capped at 4000 bytes. No truncation hides job identifiers.

Synthetic tests preview the actual Slack JSON and send once without scheduler/state.
They contain no real scheduler data. Setup identifies Slack and its disclosed fields,
showing only the safe origin, never the secret webhook path.

The private HTTPS frame retains its grammar. Its child accepts the existing generic
`event_id` binding or the exact four-field Slack shape with disabled formatting/
unfurls, bounded text and a final matching public event ID. Invalid/mismatched
bodies still fail before network activity.

Reuse HTTP acknowledgement classification (2xx accepted, 408/429/5xx retry,
Retry-After honored, other codes rejected), verified TLS, disabled redirects/proxies,
helper deadline and durable retries. Response bodies are neither read nor retained.
This records HTTP acknowledgement, not human receipt or Slack message identity.
Slack does not promise deduplication of the existing Idempotency-Key header; a lost
acknowledgement can yield duplicate channel messages. A channel is chosen through
Slack webhook installation; no channel override or bot token is added.

Controlled tests cover installed setup/validate/doctor, synthetic send/refusal/rate
limit, TLS, identical rendered retry after restart, completed work not replayed,
provider-format revision guards, bounds and allowlisted rendering. Real installed
HPC Slack monitoring, site policy and support/release acceptance remain external gates.

References: [incoming webhooks](https://docs.slack.dev/messaging/sending-messages-using-incoming-webhooks/)
and [plain text formatting](https://docs.slack.dev/messaging/formatting-message-text/).
