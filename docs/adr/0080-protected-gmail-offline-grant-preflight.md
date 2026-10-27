# ADR 0080: protected Gmail offline-grant preflight

Status: adopted for experimental testing, 2026-10-27.

## Decision

A standalone Gmail profile may add oauth_client_id. This opts into a separate
versioned protected offline grant instead of the original two-field short-lived
probe credential. The profile revision binds this client identity. Existing probe
profiles remain compatible and never gain implicit refresh behavior.

The grant contains exactly schema_version=1, client_id, client_secret, subject,
email, refresh_token, access_token, expires_at and scopes. Scopes are exactly
Gmail send, OpenID and email. No shared project/client secret is shipped. Profile
subject, verified sender address and client ID must match the grant. Scope
responses may use Google's canonical userinfo.email spelling; no mailbox reads
or broader scopes are requested. Unknown grant fields and duplicate keys fail.

`email auth refresh --profile <absolute-path>` is separate from submission. It
runs a bounded same-executable helper with fixed verified token/userinfo endpoints
and an overall profile deadline. It never submits mail or opens a browser. An
expiring token is refreshed once; a still-current token is checked through live
userinfo, with one refresh allowed after an unambiguous identity-endpoint 401.
A rejected freshly refreshed token is not refreshed repeatedly. Revoked grants,
account mismatch and malformed resources fail with static output.

A credential-level nonblocking advisory lock serializes cooperating processes,
including different state directories. Refresh-token rotation is published with
same-directory mode-0600 temporary files, file sync, expected-content checking,
atomic replacement and directory sync. Publication precedes any dependence on a
rotated grant, including identity checks. Missing replacement refresh tokens
preserve the prior token. Identity failure still blocks submission. Storage or
publication uncertainty blocks further use; a remote rotation lost before a
response/publication can require reauthorization. This cannot be made atomic
with Google's server. Config's no-overwrite publication is unchanged.

The workstation-only `scripts/gmail-authorize.py` delegates state/PKCE/loopback
consent to Google's official OAuth library. It uses an operator-owned Desktop
client and offline consent, a bounded callback/overall flow, live verified identity
and new-only protected grant publication. Its optional Python library is a
workstation setup dependency, not an HPC runtime dependency. It logs no OAuth URL,
code or credential and sends no email. It is Unix-only, like protected publication.

`email test` can use an unexpired bound offline grant but does not automatically
refresh. The operator runs auth refresh explicitly first. API acceptance is not
inbox receipt. Foreground integration is specified separately in
[ADR 0081](0081-gmail-foreground-preflight-and-cooldown.md).

## Evidence

Controlled TLS tests cover expiry, cached token reuse, refresh rotation/omission,
revoked grants, mismatched verified identity and one bounded refresh after 401.
Protected-file tests cover exclusivity, stale expected contents, atomic replacement,
write-sync failure and permissions. CLI tests prove separate resource-free help,
private failure output, no state creation and no email on a grant/profile mismatch.
Workstation helper tests use synthetic credentials and never open a browser.
Real OAuth consent, account receipt and production token policy acceptance remain
operator gates. External Google projects in Testing can issue short-lived refresh
grants; no unattended support claim follows from this experiment.
