# Local SMTP configuration v1 extension proposal

> Adopted for the local MVP on 2026-10-13. Statements below describing this proposal
> as unapproved are historical draft context. The [adoption record](local-contract-review-v1.md)
> defines scope; installed implementation and acceptance remain separate milestones.

Status: draft for review. This extends the [local configuration proposal](local-webhook-config-v1.md)
to describe the SMTP portion of the required MVP. No accepted schema, file loader,
credential lookup or installed transport is introduced by this document.

See the [synthetic mixed configuration](examples/local-mixed-config.json). Its
permission tokens are placeholders, not computed or valid authorization for a
real profile. Example numbers are review values, not operational defaults.

## Proposed provider fields

All fields below are required, except where stated. Reject inline passwords,
unknown keys, invalid types, duplicate keys and unsupported provider/TLS modes.
Resolve local resources before polling or sending; emit static errors, never values.

| Field under `providers.<name>` | Proposed meaning |
| --- | --- |
| `type` | Constant `smtp` |
| `relay` | Explicit ASCII DNS name or IP address; no URL, userinfo or implicit relay discovery |
| `port` | Integer 1..65535 |
| `tls_mode` | Constant `required_starttls` for the initial activated path; implicit TLS follows verified supervised acceptance |
| `server_name` | Explicit certificate verification DNS name or IP; may differ from relay address; never disable verification |
| `trust_der_files` | 1..32 absolute local paths to DER trust anchors; at most 65,536 total DER bytes; no automatic system-root fallback |
| `authentication.identity` | Expected AUTH username, bounded to 64 ASCII bytes by the current private credential rules; private configuration metadata |
| `authentication.username_env` | Environment-variable name resolving to exactly the expected identity |
| `authentication.password_env` | Environment-variable name resolving to a nonempty password of at most 128 ASCII bytes under current private rules |
| `sender` / `recipient` | One explicit address each, bounded to 254 ASCII bytes under the prototype address validation; multi-recipient messages are deferred |
| `visibility` | Required `hidden` or `included`; determines whether allocation/task identifiers enter SMTP content |
| `identity_namespace` | Explicit local rendering namespace; proposed 1..64 ASCII letters/digits/underscore/hyphen |
| `request_timeout_milliseconds` | Whole integer 1..30,000; absolute parent deadline for DNS, stdin, TLS, authentication and SMTP submission |

DER files are explicit trust configuration, not credentials. Require regular,
bounded files and deliberate trust-source review; reject symlinks and duplicate
anchors, read bounded bytes, parse certificates and fingerprint actual contents.
PEM/system stores and enterprise trust loading need separate parser/support review.
The ASCII username/password constraints are a current implementation limitation:
review whether to retain them before freezing a product contract. No alternate auth
mechanism or unauthenticated fallback is proposed.

Use the common environment-name grammar from the webhook proposal. Transfer secret
values to supervised children through bounded zeroizing stdin, with a cleared child
environment and no secret argv. Persist neither passwords nor resolved secret
values. Identifiers and credential references remain sensitive local metadata.
Environment rotation works after restart under the same identity/reference; no live
reload or background secret refresh is proposed.

## Pending work and permission policy

Atomically freeze the SMTP envelope/DATA, rendering descriptor and provider descriptor
with the event/outbox. Bind relay, port, TLS mode/name, actual trust fingerprint,
authentication identity and credential/trust references. Changed configuration
applies only to new events. Pending work retains its old profile and bytes.

Propose `delivery.smtp_permissions` as a required array when SMTP providers exist.
Each entry has `cluster_id`, `destination`, `rendering_revision` and
`provider_revision`; revision values are 64 lowercase hexadecimal digits. At most
64 distinct entries; an empty list denies all SMTP work. Permissions are checked
before resolution, network access and exhausted-budget retirement. Do not infer
permission from a reused provider name, silently approve a changed pair, downgrade
SMTP work to ordinary delivery or accept invalid stored provenance.

The wizard should compute current tokens, explain the selected disclosure and
endpoint, then enroll the exact pair only after setup confirmation. It must not
require users to calculate hashes. Validation should distinguish current approval
from pending historical profiles that still need their original approved resources.
The precise workflow for approving historical profiles is a review decision;
changing or deleting a destination must block unresolved pending work, preserving
it for explicit reconciliation. No queue-discard or retarget command is proposed.

Message-ID remains stable across attempts. SMTP relay acceptance is not human
receipt. If acceptance precedes a lost acknowledgment/local commit, at-least-once
replay may duplicate mail. Preserve final-phase evidence and bounded retry policy;
auth/certificate/configuration blocks must not consume an invented send attempt.

## Review and acceptance

Review trust sources, ASCII/auth constraints, address/disclosure limits, required
fields, permission enrollment UX and safe historical-profile resource handling.
After approval, define schema/examples and installed tests for bounded file/env
loading, atomic preparation, TLS verification, secret rotation, changed endpoint
blocking, pending-record preservation, restart and partial mixed-route progress.
The DER/permission workflow prioritizes explicit provenance but needs usability
validation against the five-minute onboarding target. Do not claim that target is met.
