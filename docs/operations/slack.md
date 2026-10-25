# Slack on the HPC

Slack incoming webhooks use HTTPS, so they may work where outbound SMTP is blocked.
The operator has confirmed a separate Python test posted a message from uop-hpc.
The operator subsequently confirmed the installed Rust synthetic Slack test too.
The operator also confirmed healthy monitoring and completed array-task alerts
using the original message format. Detailed messages still need HPC acceptance.
This does not establish a supported platform or authorize real scheduler polling.

Create a Slack app in your workspace, enable Incoming Webhooks, and add a webhook
for your intended channel using [Slack's instructions](https://docs.slack.dev/messaging/sending-messages-using-incoming-webhooks/).
The webhook URL is a credential. Never put it in a config, command argument,
repository, screenshot or support report. The channel is selected in Slack.

From the updated repository, build with `cargo build --release --locked`. In Bash,
load the URL at a hidden prompt (no leading/trailing whitespace):

```bash
read -r -s -p "Slack webhook URL: " SLURM_NOTIFY_SLACK_WEBHOOK_URL
printf '\n'
export SLURM_NOTIFY_SLACK_WEBHOOK_URL
```

Create a fresh protected draft and separate state path. This leaves Gmail resources
available for later use. Run from the repository root. Discover Slurm ClusterName
first; it is a scheduler identity and may differ from the login hostname. Do not use a freely chosen label.

```bash
export SN_SLURM_CLUSTER="$(timeout 10s scontrol show config | awk '$1 == "ClusterName" {print tolower($3)}')"
printf 'Slurm cluster: %s\n' "$SN_SLURM_CLUSTER"
# If no single name was printed, stop and check your Slurm environment.
umask 077
mkdir -p "$HOME/.config/slurm-notify"
chmod 700 "$HOME/.config/slurm-notify"
SN_BASE="$(cd "$HOME/.config/slurm-notify" && pwd -P)"
export SN_SLACK_DIR="$(mktemp -d "$SN_BASE/slack-XXXXXX")"
python3 - <<'PY'
import json, os, re
from pathlib import Path
root = Path(os.environ['SN_SLACK_DIR'])
source = json.loads(Path('contracts/config/slack-v1.example.json').read_text())
cluster = os.environ['SN_SLURM_CLUSTER']
if not re.fullmatch(r'[A-Za-z0-9_-]{1,64}', cluster):
    raise SystemExit('Expected one valid Slurm ClusterName; no draft written.')
source['monitor']['cluster_id'] = cluster
source['storage']['path'] = str(root / 'state' / 'state.sqlite3')
draft = root / 'draft.json'
with draft.open('x') as stream:
    json.dump(source, stream, indent=2)
    stream.write('\n')
draft.chmod(0o600)
print('Draft path:', draft)
PY
./target/release/slurm-notify init --config "$SN_SLACK_DIR/config.json"
```

At the prompt enter the printed absolute draft path, review the disclosure, then
enter `yes`. Initialization sends nothing. Run validation and one synthetic test:

```bash
./target/release/slurm-notify config validate --config "$SN_SLACK_DIR/config.json"
./target/release/slurm-notify test slack --config "$SN_SLACK_DIR/config.json"
```

Expect a synthetic JSON preview, successful acknowledgement and a channel message
starting `Slurm Notify test`. Failure does not retry automatically; acceptance may
be unknown, so inspect the channel before repeating. Do not share the webhook URL.
The installed client uses bundled public TLS roots and direct HTTPS, without proxy
environment variables; Python connectivity alone does not prove this path works.

After validating the site's scheduler polling/execution policy, run:

```bash
./target/release/slurm-notify doctor --config "$SN_SLACK_DIR/config.json"
./target/release/slurm-notify start --foreground --config "$SN_SLACK_DIR/config.json"
```

The terminal must stay open; Ctrl-C stops monitoring. Defaults poll every 60 seconds,
suppress initial existing job observations, and route terminal lifecycle events.
Doctor probes scheduler versions and checks local identity when sibling scontrol
is available; it cannot determine site policy. Test a newly
submitted job only when permitted by that policy. Reload the URL and directory
variables in new shells; do not start multiple owners for the same state path.

The current example enables detailed notifications: state icons, cluster identity,
separate allocation/array-task IDs, duration, allocation exit status:signal, validated
UTC start/end times, observation time and event ID. Unknown values say unavailable.
Job names are hidden unless explicitly enabled. Scripts, logs and user names are
not included. Messages disable
Markdown/unfurls. The canonical event and rendered request must each fit the configured
payload byte limit; Slack text additionally has a 4000-byte cap, with no truncation.

Slack shares the durable HTTPS retry behavior: 408/429/5xx retry (including
Retry-After); other non-2xx responses reject the attempt. A lost acknowledgement may
produce a duplicate. The HTTP response body is not inspected; success means an HTTP
acknowledgement, not verified channel viewing. Format/URL/trust changes block pending
work; do not edit a provider type over a pending outbox to bypass that guard.

## Troubleshooting first monitoring startup

Doctor probes Slurm versions and optionally reads ClusterName through scontrol
beside canonical squeue. It does not query job/accounting data or prove access. A successful synthetic Slack test establishes provider
delivery, not scheduler compatibility or permission.

Command discovery currently accepts at most 4096 PATH bytes and 64 absolute entries.
An oversized Spack PATH produces the same unavailable-executables message as missing
commands. If `command -v squeue` and `command -v sacct` both resolve under `/usr/bin`,
use a per-command override rather than changing your shell or unloading modules:

```bash
PATH=/usr/bin:/bin ./target/release/slurm-notify doctor --config "$SN_SLACK_DIR/config.json"
# Only after site polling/execution policy is established:
PATH=/usr/bin:/bin ./target/release/slurm-notify start --foreground --config "$SN_SLACK_DIR/config.json"
```

If startup succeeds but the cycle stops, retain state and run
`./target/release/slurm-notify status --config "$SN_SLACK_DIR/config.json"` while the
monitor is stopped. An empty checkpoint means no completed poll was recorded; it
does not prove permission denial. Updated builds print safe `stage`/`reason` tokens
before the final runtime message. Share those tokens, exit code and historical
status rather than credentials, raw scheduler output or database contents. The
`policy` category describes an application validation check, not site authorization.

## Upgrade existing Slack alerts to details and icons

Existing configs keep v1 rendering until opted in. Stop monitoring with Ctrl-C,
then run status. Drain pending deliveries with the old configuration first:
queued/due/retrying must be zero and the sample must not be truncated. Changing
renderer or name disclosure over pending work blocks it rather than reinterpreting it.

Take a closed backup (monitor stopped, no other owner):

```bash
umask 077
SN_BACKUP_DIR="$(mktemp -d "$SN_SLACK_DIR/before-details-XXXXXX")"
cp -p "$SN_SLACK_DIR/config.json" "$SN_BACKUP_DIR/config.json"
cp -a "$SN_SLACK_DIR/state" "$SN_BACKUP_DIR/state"
printf 'Closed backup: %s\n' "$SN_BACKUP_DIR"
```

Create a new protected draft from the working config. The following explicitly opts
in to names; set `include_job_name` to `False` to keep names hidden. Names remain
in immutable protected state/backups for the ledger's lifetime, even after outbox
retention cleanup. They are never added to generic webhook or SMTP messages.

```bash
python3 - <<'PY'
import json, os
from pathlib import Path
root = Path(os.environ['SN_SLACK_DIR'])
source = json.loads((root / 'config.json').read_text())
source['providers']['slack']['include_job_details'] = True
source['providers']['slack']['include_job_name'] = True
with (root / 'details-draft.json').open('x') as stream:
    json.dump(source, stream, indent=2)
    stream.write('\n')
(root / 'details-draft.json').chmod(0o600)
print('Draft path:', root / 'details-draft.json')
PY
./target/release/slurm-notify init --config "$SN_SLACK_DIR/details-config.json"
```

Enter the printed absolute draft path, review the fields/name disclosure/retention,
and type yes. Keep the working cluster identity and state path. From now on use
the newly reviewed `details-config.json`:

```bash
PATH=/usr/bin:/bin ./target/release/slurm-notify doctor --config "$SN_SLACK_DIR/details-config.json"
PATH=/usr/bin:/bin ./target/release/slurm-notify start --foreground --config "$SN_SLACK_DIR/details-config.json"
```

The first selected detailed event transaction upgrades state to schema v6 and
freezes the per-destination Slack JSON. Old alerts are not rewritten or resent.
Old binaries reject v6: downgrade requires stopping and restoring the closed backup
and its configuration. An older checkpoint can replay subsequent alerts. Never
lower user_version manually or copy a live SQLite database.

Details come from the existing bounded local current-user accounting query with
UTC/C locale enforced. ExitCode describes the allocation script, not necessarily
all steps. Names are bounded and Slack special characters escaped. Unknown optional
values show unavailable; newline-ambiguous scheduler records remain parse failures.
Memory/partition statistics, array summaries and rich footer layouts are later work.

Unknown accounting details remain unavailable in the frozen alert; a retry does
not refresh them. Later accounting availability does not rewrite an earlier alert.
