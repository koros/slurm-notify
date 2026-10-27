# ADR 0077: explicit supervised HTTPS email tests

Status: adopted for the first implementation increment, 2026-10-27.

The user authorized implementation of the HTTPS email plan. Start with the bounded
one-shot feasibility boundary and shared encoders. Do not add email API monitoring
until transactional request freezing, API-specific permissions and durable pause/
resume are implemented. This ADR adopts the test boundary only; issue #19 remains
open for monitoring/persistence/OAuth lifecycle contracts.

## Public boundary

Add `email test --profile <absolute-path>` and resource-free `email --help`.
The separate protected profile is not a normal monitor configuration. Parse strict
JSON with duplicate/unknown key rejection and a 16 KiB cap. Schema version is 1.
Required common keys: provider, sender, recipient, account_identity,
request_timeout_seconds (1–30) and payload_limit_bytes (1024–16384). Addr-specs
use the existing strict ASCII parser; no display names, lists or header injection.

- sendgrid: require api_key_env; fixed US v3 Mail Send endpoint.
- mailgun: require api_key_env, lowercase DNS domain and explicit region us/eu;
  fixed regional v3 messages endpoint. No endpoint overrides or regional failover.
- gmail_api: require an absolute credential_file; account_identity is the Google
  OIDC subject. Sender must equal the verified Google account email.
- Optional trust_der_files replaces bundled public roots using existing protected
  snapshot rules. No system-trust fallback, redirects or ambient proxy discovery.

API keys are named environment resources, not inline JSON. Gmail credentials are
protected strict JSON with exactly access_token and expires_at (integer Unix UTC
seconds through 2099). A token must remain valid for the complete configured
request deadline plus a 30-second margin. No refresh token, client secret, browser
flow or automatic refresh is accepted in this first format. Operators obtain the
token using approved workstation OAuth with gmail.send plus minimal OIDC identity
scopes. The helper calls only userinfo and messages.send; no mailbox read API.
A successful live userinfo response must bind subject, email and email_verified.

## Execution and data

Snapshot profile, credentials and optional DER roots under existing ownership,
no-symlink and byte-limit rules. Supply them only on the owned child's stdin;
clear its environment and bound input/output/deadline with existing supervision.
No configurable helper path, secret argv, raw HTTP errors or response logging.
The private five-field frame is profile bytes, credential bytes, UUID, synthetic
Unix timestamp and encoded DER roots, with total limit 103000 bytes. It does not
alter existing webhook/SMTP helper grammars. Child validates every field again.

Send exactly one labelled synthetic email request, after optional Gmail identity
verification. No Slurm, SQLite, monitoring, automatic resend, token refresh or
provider fallback. The parent previews only synthetic content, provider kind,
public event ID and a secret-free profile-reference fingerprint. No addresses,
account identity or credential values are printed. Profile fingerprint binds
metadata/resource references; it is not durable permission enrollment.

Shared content uses canonical minimal job fields in controlled tests and a distinct
synthetic message for installed testing. Keep Slack metadata out. SendGrid encodes
JSON and disables supported tracking. Mailgun freezes deterministic multipart,
tracking-off options and Message-ID. Gmail uses the existing email builder, removes
SMTP DATA dot transparency/terminator, and encodes real MIME as base64url JSON raw.
Encoded bytes must fit the profile cap. Stable Message-ID does not promise deduplication.

## Results and limits

Accept only SendGrid 202, Mailgun 200 with a bounded ID, or Gmail 200 with a bounded
ID. Error/receipt parsing is limited to 8 KiB, duplicate-free bounded-depth JSON;
compressed responses are not decoded. SendGrid 202 does not need a response body.
The underlying client bounds response header count/line size; the supervised
process bounds total time including DNS, identity and send. There is no SDK retry.

Private acknowledgements distinguish accepted, rejected, unknown, retry delay,
authentication, policy and not-attempted. Gmail allowlisted quota reasons can
request backoff; do not reject every 403 permanently. Retry-After is never capped
to an earlier backoff; a valid time beyond 2099 requires operator review. Explicit
testing never acts on a retry hint. Unknown success/transport results report
acceptance uncertainty, not inbox failure. Userinfo activity can happen even when
no email request is submitted; test preflight does not claim zero network activity.

CLI exits: 0 for API acceptance, 1 for an unacknowledged/refused/not-submitted test,
2 for invalid invocation/profile/resources. All diagnostics are static. Real inbox
receipt remains an operator gate. No new runtime dependency, polling/default,
normal config field, permission baseline or storage migration is introduced.

References: [plan](../proposals/https-email-delivery-v1.md),
[SendGrid](https://www.twilio.com/docs/sendgrid/api-reference/mail-send/mail-send),
[Mailgun](https://documentation.mailgun.com/docs/mailgun/api-reference/send/mailgun/messages/post-v3--domain-name--messages),
[Gmail](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/send).
