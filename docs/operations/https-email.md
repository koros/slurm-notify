# HTTPS email delivery (prototype)

The installed `email test` command sends one synthetic email through SendGrid,
Mailgun or Gmail API over verified HTTPS. It does not monitor jobs, create state,
refresh OAuth tokens or retry automatically. SendGrid and Mailgun also support
enrolled foreground monitoring. Gmail API remains a separate one-shot test until
OAuth lifecycle acceptance. See the [roadmap](../architecture/https-email-delivery-roadmap.md).

## Before sending

Confirm site/account policy and access from the intended monitor node. Use an
operator-controlled sender and recipient. Obtain the account prerequisites:

| Provider | Prerequisites |
| --- | --- |
| SendGrid | Restricted Mail Send key and verified sender. Initial scope uses the US API. |
| Mailgun | Domain Sending Key and matching US/EU domain; sandbox recipients must be authorized. |
| Gmail API | Enabled API, approved Desktop OAuth client/consent and a short-lived access token with gmail.send, openid and email identity access. |

An incoming Slack webhook working does not establish these endpoints are reachable.
An API success does not establish inbox receipt. Check the destination inbox and
spam manually; no mailbox-read permission is needed.

## Protected test profile

Build with `cargo build --release --locked`. Copy one of the separate examples in
[contracts/config](../../contracts/config/README.md#standalone-https-email-test-profiles)
into a directory owned by you with mode 0700; make the profile mode 0600. Edit the
example addresses and account/resource references locally. Do not put API keys,
OAuth tokens, webhook URLs or private account metadata into issues/screenshots.

Profiles require explicit timeout/payload budgets and reject unknown/duplicate keys.
Do not paste one into the normal `providers` map. Optional protected DER roots
replace bundled public trust; certificate verification cannot be disabled.

For SendGrid/Mailgun, set the environment variable named by api_key_env using a
hidden prompt, for example in Bash:

```bash
read -r -s -p 'Email API key: ' SLURM_NOTIFY_EMAIL_API_KEY
printf '\n'
export SLURM_NOTIFY_EMAIL_API_KEY
```

Use a sending key with the narrowest provider permission, never an administration
key. Environment exports last only for the current session and its children.

## Gmail feasibility credentials

Use Google's [supported workstation OAuth flow](https://developers.google.com/identity/protocols/oauth2/native-app)
with an operator-owned Desktop client, state/PKCE and a loopback callback. Do not
start an OAuth listener on the HPC or use deprecated out-of-band code pasting.
Request gmail.send and the minimal OpenID email identity scopes. Review the
[OAuth design requirements](../proposals/https-email-delivery-v1-design-notes.md#gmail-oauth-and-early-testing).

The credential_file points to a protected JSON file with exactly:

- access_token: the short-lived OAuth access token, never an SMTP app password.
- expires_at: its actual expiry as integer Unix UTC seconds, not a guessed lifetime.

Create it with mode 0600 in a protected directory and transfer it using an approved
secure method. Do not place tokens in shell arguments or history. Set profile
account_identity to the Google subject and sender to its verified account email
from that authorization. Userinfo is checked over verified HTTPS before sending;
a mismatch prevents submission. Do not copy an email address into the subject field.

The token must have enough remaining lifetime for the profile timeout plus 30 seconds.
The test does not request broader scopes, read the mailbox or refresh credentials.
Revoke/discard probe credentials after the experiment. Google Testing grants and
public client distribution have additional expiry/policy constraints; this is not
an unattended Gmail notifier.

## Send one explicit test

```bash
./target/release/slurm-notify email --help
./target/release/slurm-notify email test --profile /absolute/private/email-test.json
```

The command previews labelled synthetic content and a public event ID, then runs a
bounded same-executable helper with protected snapshots on stdin. Credentials do
not appear in argv or the helper environment. Gmail may make a separate userinfo
request; there is at most one email submission and no automatic resend.

Exit 0 means exact provider-specific API acceptance. Exit 1 means rejected,
not-submitted or uncertain acceptance; inspect the static diagnostic and check the
inbox before repeating. Exit 2 means invalid invocation/profile/resources. A timeout
or lost acknowledgement can conceal a successful submission, so repeating a test
can create duplicate emails. Rate-limit hints are classified but never retried by
this command.

Ordinary tests use controlled TLS/API fixtures and fake credentials. Operator
receipt from a real provider is still pending and must be recorded per tested
node/account/region. Keep this experiment separate from real scheduler monitoring.

## SendGrid and Mailgun foreground monitoring

Start with `sendgrid-v1.example.json` or `mailgun-v1.example.json` from
[the config examples](../../contracts/config/README.md). These are full monitoring
drafts, distinct from standalone test profiles. Set the real Slurm cluster name,
private state path, sender, recipient and API-key environment reference. The
account_identity is an operator-assigned label; it does not authenticate the key's
account. Mailgun's domain and region must match your provider account.

Protect the draft as mode 0600 in a user-owned 0700 directory. Leave
api_email_permissions empty initially. Run:

```bash
./target/release/slurm-notify init --config /absolute/private/email-config.json
# Enter the protected draft path and review the displayed addresses/disclosure.
./target/release/slurm-notify config validate --config /absolute/private/email-config.json
./target/release/slurm-notify test primary --config /absolute/private/email-config.json
./target/release/slurm-notify doctor --config /absolute/private/email-config.json
# After checking site polling/execution policy:
./target/release/slurm-notify start --foreground --config /absolute/private/email-config.json
```

Load the referenced sending key before validation/test/start. Enrollment itself
needs no API key and sends nothing. `init` displays private addresses and account
metadata for review: keep that terminal output out of public logs/screenshots.
Optional trust files replace bundled public roots; verification remains required.

Only current-user jobs and explicitly routed events are monitored. Minimal emails
include state, job identifier, cluster, observation time and stable public event
ID. No job name or Slack enrichment is automatically disclosed. Frozen requests
contain addresses/content and persist in private state for its lifetime; protect
backups too. SMTP approval does not authorize API-email disclosure.

Missing credentials, authentication failures and provider-policy refusals pause
only the affected API destination. Stop the monitor, correct the cause, inspect
historical status and explicitly clear the pause before restarting:

```bash
./target/release/slurm-notify status --config /absolute/private/email-config.json
./target/release/slurm-notify provider resume primary --config /absolute/private/email-config.json
```

Recovery needs the exact enrolled profile and stopped existing schema-v7 state.
It sends nothing and preserves attempts, due times and exact frozen requests.
Changing a recipient/domain/account/trust profile cannot silently reroute pending
mail. Re-enroll changed settings into a new protected config and retain old state
for deliberate disposition; do not edit database rows to force compatibility.
Retries can duplicate a remotely accepted message after an acknowledgement or
local commit failure. A stable Message-ID helps investigation, not guaranteed
provider deduplication. Check manual inbox receipt before claiming deployment
acceptance. SendGrid/Mailgun real-account acceptance is still pending.
