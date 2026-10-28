# ADR 0081: Gmail foreground preflight and provider cooldowns

Status: adopted for the experimental local prototype, 2026-10-27.

## Decision

Normal provider type gmail_api requires the standalone offline-profile fields,
using type instead of provider and no nested schema_version. oauth_client_id is
mandatory. The versioned grant in ADR 0080 is required; original short-lived probe
credentials remain valid only with the separate standalone probe profile.

Gmail uses ADR 0079 API-email permissions. Enrollment binds client, subject,
verified sender, recipient, credential reference, trust, budgets and minimal content.
Setup displays the binding for review. Config validation/doctor check protected
local grant identity and scope shape without refresh or provider requests. An
expired local access token is valid configuration; runtime preflight refreshes it.
Runtime checks the exact frozen revision before resolving credentials or preflight.

Before a Gmail submission, a separate bounded helper obtains a valid access token,
refreshes when needed, saves rotations and verifies live subject/verified email.
Each helper has its own request_timeout_seconds deadline; preflight plus
submission uses at most twice that configured helper budget, plus process cleanup
and synchronous OS-call latency. The parent rereads the protected matching grant. Only its access token enters the
separate email-submission helper, which makes exactly one fixed-endpoint POST and
no identity/token requests. Thus OAuth and identity work never consume email-send
attempts. No mailbox read API or sender alias is used.

A local credential problem or permanent identity/authentication/policy failure
pauses only its destination without a fabricated send attempt. Temporary preflight
failures defer the provider without an attempt. Valid Retry-After hints are honored
from preflight completion and never shortened; otherwise the existing retry delay
and jitter provide the next eligibility time. Other destinations keep progressing.
Cancellation is rechecked after authorization to prevent a POST after a stop signal
received during preflight. Cleanup/timeout supervision still bounds the child.

Schema v8 adds email_provider_cooldowns (cluster, destination, revision, retry_at),
created lazily in the first deferred API preflight transaction. Cooldowns filter
due work before batch limits, cover newly observed jobs and are rechecked within
the batch. They survive restarts without resetting attempts or changing requests.
Updates never shorten a cooldown. An expired old revision can be replaced only
when no incompatible pending work remains. A late migration/write failure rolls
back. Existing schemas 4–7 remain readable; explicit recovery accepts stopped
schemas 7–8 and preserves cooldown timing. Historical status adds bounded counts.

## Deliberately conservative authentication recovery

An identity-endpoint 401 permits one refresh during preflight (ADR 0080). A Gmail
**messages.send** 401 instead records the rejected dispatch and immediately pauses
for explicit operator correction/resume, like SendGrid/Mailgun. It does not
silently refresh and resend. This narrows the proposal's optional automatic 401
recovery to avoid cross-file/outbox recovery markers and repeated sending under a
revoked/reduced-scope grant. Reauthorization, exact resume and preserved retry
budgets are the v1 recovery contract. No immediate email resend occurs.

Local `email auth status` inspects protected bound grant metadata without network
access. `email auth logout` takes the credential lease, verifies the binding and
removes only the local grant with a directory sync; Google consent must be revoked
separately. Neither operation changes outbox state. Stopped consumers and deliberate
workstation reauthorization replace implicit browser login in monitoring.

Resume never revives exhausted deliveries or resets attempt budgets. There is no
public exhausted-delivery requeue action in v1; any future requeue needs its own
reviewed delivery-generation contract.

## Compatibility and acceptance

The generic event schema, original provider wire protocols and legacy payload
bindings do not change. API email is minimal; Slack details/mentions never leak
into its content. Take a stopped protected backup before enabling new providers;
old binaries reject newer state rather than interpreting it. Downgrade requires
deliberate restoration/disposition, since restoring old delivery history can replay
emails accepted since the backup. Never edit SQLite to lower user_version.

Controlled tests cover Gmail MIME/API dispatch, refresh/identity and token rotation,
permissions, frozen bytes, restart, provider isolation, cooldowns including new jobs,
preflight cancellation and migration rollback. Synthetic helpers use fake grants;
no Google account is contacted in ordinary tests. Real workstation consent, Gmail
inbox receipt, deployed job notifications and SendGrid/Mailgun account/region
acceptance remain explicit external gates, not implied support or a release.
