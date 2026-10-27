# ADR 0079: API email enrollment and explicit recovery

Status: adopted for the local prototype, 2026-10-27.

## Decision

Normal schema-v1 configs accept explicit `sendgrid` and `mailgun` provider types.
Their required fields match the standalone test profiles, replacing `provider`
with `type` and omitting the nested schema version. Mailgun requires a domain and
US/EU region. Gmail foreground remains rejected until protected refresh lifecycle
acceptance. Existing SMTP/webhook/Slack meanings and event schemas are unchanged.

`delivery.api_email_permissions` is required for API-email configs. Each entry
contains exactly cluster_id, destination, rendering_revision and provider_revision.
`init` reviews the actual sender, recipient and operator-assigned account identity,
then enrolls only the current API-email pairs on explicit confirmation. SMTP
permissions cannot authorize API email. API keys are not resolved during enrollment.
Changes to addresses, account identity, key reference, domain/region, transport
budgets or trust snapshot require new enrollment; rotating the value behind the
same key reference does not. An API key's actual account cannot be verified by the
local account label: operators must use an appropriately restricted sending key.

Configured `test <alias>` sends one synthetic alert without state or retries.
`config validate`, `doctor` and foreground startup enforce enrollment. Foreground
credential lookup follows the exact frozen route/profile guard. Minimal API-email
content contains state, job identifier, cluster, observation time and public event
ID. Job names and Slack detail options are not inherited.

## Persistence and recovery

ADR 0078 frames are committed with observation/checkpoint/outbox writes. Retries
use the same exact encoded request and Message-ID. Provider acceptance is not
inbox delivery, and ambiguous transport/commit failures can produce duplicates.

A missing/malformed local API key pauses its destination without creating a send
attempt. A submission returning authentication or provider-policy rejection records
one attempt and a durable pause atomically. Other transient/uncertain results use
the existing bounded retry policy; rate-limit times are never shortened. A send
attempt records a supervised submission dispatch, not proof of remote receipt.

Paused destinations are filtered before due-batch limiting and rechecked within
the batch. Other destinations continue. `status` adds a bounded API pause count;
it reports historical state, not live process health.

`provider resume <alias> --config <absolute-path>` requires a stopped, protected,
already-existing schema-v7 state and the exact enrolled revision. It clears only
that pause, refuses incompatible pending revisions, and never creates/migrates
state, resolves credentials, calls Slurm, sends a request, changes due times,
rewrites frozen content or resets attempt budgets. Run it after correcting the
key/account/policy problem, then restart. A changed profile requires separately
reviewed pending-work disposition; this command cannot reroute old notifications.

## Validation and limitations

Controlled TLS tests establish endpoint/authentication/encoding/result behavior.
Synthetic storage/worker tests establish atomic preparation, integrity, restart,
provider isolation and preserved retry budgets. Real account acceptance and inbox
receipt remain separate pending gates. No supported platform or release is claimed.
