# Slack on the HPC

Slack incoming webhooks use HTTPS, so they may work where outbound SMTP is blocked.
The operator has confirmed a separate Python test posted a message from uop-hpc.
Installed Rust delivery still needs the explicit test below. This does not establish
a supported platform or authorize real scheduler polling.

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
available for later use. Run from the repository root; change the cluster label if
needed. The label is your stable local identifier, not automatic cluster discovery.

```bash
umask 077
mkdir -p "$HOME/.config/slurm-notify"
chmod 700 "$HOME/.config/slurm-notify"
SN_BASE="$(cd "$HOME/.config/slurm-notify" && pwd -P)"
export SN_SLACK_DIR="$(mktemp -d "$SN_BASE/slack-XXXXXX")"
python3 - <<'PY'
import json, os
from pathlib import Path
root = Path(os.environ['SN_SLACK_DIR'])
source = json.loads(Path('contracts/config/slack-v1.example.json').read_text())
source['monitor']['cluster_id'] = 'uop-hpc'
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
Doctor probes scheduler versions but cannot determine site policy. Test a newly
submitted job only when permitted by that policy. Reload the URL and directory
variables in new shells; do not start multiple owners for the same state path.

Notifications disclose cluster label, job/array identifier, state, observation time
and opaque event ID. They omit job names/scripts/logs/user names. Messages disable
Markdown/unfurls. The canonical event and rendered request must each fit the configured
payload byte limit; Slack text additionally has a 4000-byte cap, with no truncation.

Slack shares the durable HTTPS retry behavior: 408/429/5xx retry (including
Retry-After); other non-2xx responses reject the attempt. A lost acknowledgement may
produce a duplicate. The HTTP response body is not inspected; success means an HTTP
acknowledgement, not verified channel viewing. Format/URL/trust changes block pending
work; do not edit a provider type over a pending outbox to bypass that guard.
