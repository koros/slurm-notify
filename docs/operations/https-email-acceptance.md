# HTTPS email acceptance matrix

Updated: 2026-10-27. The implementation is an unreleased local prototype.
Controlled acceptance uses fake credentials, synthetic scheduler data and owned
TLS/API fixtures. It does not establish access, consent, delivery or support for
a real account, cluster, provider region or installed platform.

| Case | Controlled evidence | Real operator evidence |
| --- | --- | --- |
| SendGrid US | Fixed endpoint, Mail Send JSON, 202, TLS/auth/policy/rate/size/uncertainty paths, frozen requests | Pending |
| Mailgun US/EU | Explicit regional host/domain binding, deterministic multipart, message-ID acceptance, no failover | Pending for each region/account |
| Gmail one-shot | Deterministic MIME/base64url, bound verified identity, exact message-ID acceptance, one submission | Pending workstation consent and inbox/spam receipt |
| Gmail offline grant | Protected parsing/ownership, expiry, one refresh after identity 401, rotation/omission, rejection and atomic publication | Pending real refresh/reauthorization |
| Foreground/recovery | Enrolled routes, exact snapshots across restart, API pauses and provider cooldowns, unrelated-route progress, preserved attempts, cancellation before POST | Pending intended-node completed/failed/cancelled jobs |
| Compatibility | Legacy SMTP/webhook/Slack suite, additive schemas, lazy migration rollback, existing-only inspection/recovery | Pending deployment-specific stopped backup/restore acceptance |

## Operator-run sequence

1. Confirm the intended node and site polling/execution policy. Build the locked
   source version and record its commit ID without credentials or private paths.
2. For Gmail, prepare the operator-owned Desktop client and protected offline grant
   on the workstation using [the guide](https-email.md). On the intended HPC node,
   run explicit credential refresh, then one standalone synthetic test. Confirm
   inbox/spam receipt; API acceptance alone is insufficient. Do not retry an
   ambiguous submission before checking the inbox.
3. Prepare the full provider draft with the actual Slurm ClusterName, protected
   state, sender/recipient and explicit routes. Review/enroll with init; run config
   validate, doctor and one configured provider test. Confirm receipt again.
4. After checking policy, monitor in the foreground and submit minimal one-CPU jobs
   for completion, a nonzero exit and cancellation. Record sanitized state/event ID
   and receipt. Test restart without replaying already recorded successes. Do not
   induce production failures, node faults or excessive polling for screenshots.
5. Stop monitoring before backing up private state and credentials separately.
   Retain exact source/config/schema metadata. Exercise restore in an isolated
   private copy with controlled delivery before relying on deployment rollback.
6. Record each provider/account/region separately. Keep untested cases pending.
   Revoke Google consent separately after local logout if the experiment is over.

## Sanitized evidence template

- Source commit and UTC test time:
- Intended node/cluster (only identifiers safe to publish):
- Provider and region; account recorded privately, not in the public report:
- Consent/site authorization checked:
- Synthetic API acceptance and operator-confirmed inbox/spam receipt:
- Completed / failed / cancelled job event receipt:
- Restart and stopped backup/restore result:
- Static failure category or remaining blocker:

Never attach tokens, client secrets, authorization codes/URLs, webhook URLs, full
config/credential files, private addresses or raw provider responses. No external
email, Google consent or real-cluster test was run by the implementation suite.
A release requires separate explicit authorization and installation/support evidence.
