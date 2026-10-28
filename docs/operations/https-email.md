# HTTPS email delivery (prototype)

The installed `email test` command sends one synthetic email through SendGrid,
Mailgun or Gmail API over verified HTTPS. It does not monitor jobs, create state,
refresh OAuth tokens or retry automatically. SendGrid and Mailgun also support
enrolled foreground monitoring. Gmail API supports experimental foreground monitoring with a client-bound offline
grant and separate supervised credential preflight. See the [roadmap](../architecture/https-email-delivery-roadmap.md).

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

## Protected Gmail offline credentials (experimental)

Use an operator-owned **Desktop** OAuth client in a Gmail-enabled Google project.
Add the intended account as a test user where required. On a Unix workstation
with a browser, create a private directory, move the downloaded client JSON into
it and set the client file to mode 0600. The helper requires Python 3.10+ and the
optional official google-auth-oauthlib package in a workstation virtual environment;
this adds no Python package requirement on the HPC.

```bash
python3 -m venv /absolute/private/oauth-venv
/absolute/private/oauth-venv/bin/pip install google-auth-oauthlib
/absolute/private/oauth-venv/bin/python scripts/gmail-authorize.py \
  --client /absolute/private/google-desktop.json \
  --output /absolute/private/gmail-grant.json
```

The helper opens a local browser and a loopback callback, requests only gmail.send,
openid and email with offline consent, verifies the account through userinfo and
creates a new protected grant. It never overwrites an existing output or sends an
email. Keep OAuth URLs, codes and grant files out of logs, history and screenshots.
If publication fails, inspect the private destination before retrying. The helper
has a four-minute overall bound; a headless HPC is not its intended host.

Transfer the grant securely to the intended HPC private directory. Add
`oauth_client_id` to the standalone Gmail profile, matching client_id in the grant.
Set account_identity to the grant's subject and sender to its verified email;
credential_file points at the grant. Read these fields locally without printing
access_token, refresh_token or client_secret. Then run:

```bash
./target/release/slurm-notify email auth --help
./target/release/slurm-notify email auth refresh --profile /absolute/private/gmail-test.json
./target/release/slurm-notify email test --profile /absolute/private/gmail-test.json
```

Refresh checks the live bound identity and only refreshes an expired/expiring or
unambiguously rejected cached token. It makes no email request. Exit 0 means the
grant was ready at preflight; exit 1 means preflight/publication failed; exit 2
means invalid invocation/profile/resources. The separate test still sends at most
one message and does not refresh or resend automatically. Confirm actual receipt
in the selected inbox/spam before recording acceptance.

Refresh uses fixed direct HTTPS to oauth2.googleapis.com and
openidconnect.googleapis.com; sending also needs gmail.googleapis.com. Optional
DER roots apply to all three. Concurrent credential use can fail the nonblocking
lease; stop other operations before retrying. If consent is revoked or refresh
fails permanently, authorize a new grant on the workstation, stop consumers and
replace the private credential deliberately. Never change identity to force old
pending work through. Revoke access in your Google account when finished and
remove private local copies. Local metadata inspection and credential disposal are
explicit operations:

```bash
./target/release/slurm-notify email auth status --profile /absolute/private/gmail-test.json
# Stop credential consumers before local disposal:
./target/release/slurm-notify email auth logout --profile /absolute/private/gmail-test.json
```

Status makes no network request and does not establish live token validity. Logout
checks the client/account binding, takes the credential lease and removes only that
protected local grant; it does not revoke Google consent or change queued work.
The lock sidecar remains for safe synchronization. A publication/sync error can
leave the local operation completed but unacknowledged: inspect privately before
retrying. Browser login is provided only by the workstation helper.

Google Testing grants may expire after seven days for these scopes; switching to
production has separate consent/verification requirements. Review Google's
[Desktop OAuth flow](https://developers.google.com/identity/protocols/oauth2/native-app),
[refresh-token expiry rules](https://developers.google.com/identity/protocols/oauth2#expiration)
and the [official Python flow API](https://googleapis.dev/python/google-auth-oauthlib/latest/reference/google_auth_oauthlib.flow.html).

## Experimental Gmail foreground monitoring

Use `gmail-api-v1.example.json` as the full monitoring draft. Set the actual Slurm
cluster name, private state path, intended recipient and the grant's client ID,
subject and verified sender. Set credential_file to the protected offline grant.
Start with an empty api_email_permissions list, then run the same init, config
validate, test primary, doctor and start sequence shown above. Configuration
validation checks the local grant without refreshing or contacting Google.
Configured test/foreground delivery can perform credential preflight before their
single email POST. Preflight and submission each have the configured request timeout
(up to twice that budget in total, plus process cleanup/OS-call latency); the standalone email test command retains explicit refresh.

Temporary preflight failures create a durable provider cooldown without consuming
send attempts. Historical status reports pause and active-cooldown samples. A stop
arriving during preflight prevents the later email POST. Permanent grant/account
problems pause the destination. Correct or reauthorize the grant, stop monitoring,
run provider resume for the exact enrolled alias and restart. A messages.send 401
pauses immediately for explicit recovery; it does not refresh and resend silently.
Identity-endpoint 401 recovery is limited to one refresh during preflight.

Back up stopped private state and credentials separately before upgrading. First
API content uses schema v7; the first temporary credential-preflight deferral
transaction upgrades state to v8. Old binaries reject the newer database. Restore
an earlier backup only after reviewing accepted mail since that backup, because
restored delivery history can produce duplicates. Never lower user_version or edit
frozen content. Credential rotation uses a separate file lock and atomic synced
replacement. An interrupted or failed remote rotation can still require fresh
workstation consent; do not repeatedly send while investigating.

Gmail remains experimental. This implementation has controlled acceptance only;
real OAuth consent, inbox/spam receipt and authorized HPC dummy-job delivery need
operator confirmation. SendGrid and Mailgun similarly need their own account and
region acceptance. No installed release or supported platform matrix is claimed.
