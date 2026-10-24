# Local MVP getting started

The local command set is implemented for the Linux/macOS prototype. Controlled
fixtures verify HTTPS, required STARTTLS SMTP, mixed delivery and durable restart.
This is an unreleased source build, not a supported cluster/platform release.
Review [support and acceptance gates](support-and-acceptance.md) before deployment.

## Build and place the executable

From a checked-out commit with the pinned Rust development toolchain installed:

```sh
cargo build --release --locked
./target/release/slurm-notify --version
```

The executable is self-contained with respect to its provider helpers: it launches
the same executable using private bounded stdin protocols. Keep that executable
available throughout a run. You may copy it into a user-owned binary directory;
no service or privileged installer is required. Do not replace it while monitoring.
`--help` and command help are resource-free.

## Prepare explicit settings

Create a private directory owned by your account, with mode 0700, under trusted
ancestors. Configuration and custom DER trust files must be ordinary mode-0600
files owned by that account; symlinks are rejected. Use canonical absolute paths.
For example, from the repository checkout:

```sh
install -d -m 700 "$HOME/.config/slurm-notify"
install -m 600 contracts/config/webhook-v1.example.json \
  "$HOME/.config/slurm-notify/draft.json"
```

For SMTP or both transports, start from
[the mixed example](../../contracts/config/local-mixed-v1.example.json) instead.
Edit the draft before setup. Replace example paths, cluster alias, providers,
routes and explicit operational values. Set storage to an absolute path ending
in `state.sqlite3`, such as a `state/state.sqlite3` child of your private directory.
Only the final state directory is created by start; other ancestors must exist.
Examples contain synthetic values, not site-policy recommendations or defaults.

Review every polling, accounting, retry, retention and resource limit against your
site's guidance. Choose `initial_observation` deliberately: `suppress` avoids alerts
for already-observed history; `emit` may send selected existing history. Empty
routes mean no alerts. The minimal webhook payload includes cluster/job identifiers;
SMTP job disclosure is explicitly `hidden` or `included`.

Populate the named environment references using your usual secret-handling method.
Do not put secret URLs, headers or passwords in JSON, command arguments, shell
history, bug reports or logs. Webhooks require HTTPS and use bundled public roots
unless `trust_der_files` explicitly replaces them with a protected DER set. SMTP
requires verified STARTTLS, explicit DER trust and the configured authentication
identity. SMTP credentials are not needed to review/enroll a draft, but validation
and sending need them. See [configuration semantics](../../contracts/config/README.md).

## Review and create a configuration

```sh
./target/release/slurm-notify init \
  --config "$HOME/.config/slurm-notify/config.json"
```

Run this in a terminal. Enter the draft's canonical absolute path when prompted.
Setup displays safe provider identity, transmitted fields, disclosure/observation
policy, empty-route warnings and exact current SMTP permission pairs. Type `yes`
only after reviewing them. It creates a new protected file atomically and refuses
existing files, symlinks and concurrent replacement. It creates no parent
directories and fills no operational defaults. Historical SMTP permission pairs
are not copied into the new file. Any other complete answer cancels without writing.
Setup sends nothing, reads no SMTP credentials and starts no monitoring.

If a write/sync failure is reported, inspect the destination before retrying: a
complete file may already have been published. A killed setup can leave an
unpublished private `.slurm-notify-init-*` temporary file; inspect it while setup is
stopped before removing it. Never treat it as an automatically approved config.

## Validate and diagnose

```sh
./target/release/slurm-notify config validate \
  --config "$HOME/.config/slurm-notify/config.json"
./target/release/slurm-notify doctor \
  --config "$HOME/.config/slurm-notify/config.json"
```

Validation checks protected resources and exact current SMTP permission before
credentials. Doctor also resolves `squeue` and `sacct` from an absolute-directory
PATH and runs bounded `--version` probes. It creates no state and sends no alerts.
Its success does not establish site permission, accounting completeness or supported
Slurm behavior. State health/ownership is deliberately not inspected by doctor.
Environment/NSS/filesystem startup operations do not have a universal hard deadline.

## Explicitly test a destination

```sh
./target/release/slurm-notify test primary \
  --config "$HOME/.config/slurm-notify/config.json"
```

This command authorizes one real synthetic request to the selected alias; use only
your intended destination. It previews labelled synthetic content before sending,
queries no scheduler and creates no state. SMTP still requires the exact enrolled
pair. There is no automatic retry. Success means acknowledgment by the service or
relay, not human receipt. Failure can leave acceptance unknown; avoid blindly
repeating it. Synthetic webhook tests use a separate `TEST` payload with no real
cluster/job data; monitoring events use the frozen lifecycle payload schema.

## Monitor and inspect recovery

After site polling/execution policy and intended provider use have been authorized:

```sh
./target/release/slurm-notify start --foreground \
  --config "$HOME/.config/slurm-notify/config.json"
```

The process monitors the effective user's jobs in one configured cluster, owns
private SQLite state and stays in the foreground. Ctrl-C exits 130; SIGTERM exits
143 after bounded active stages and state closure. No daemon/service is installed.
Health output labels sampled counts and truncation; it is not an exact queue total.
Static diagnostics omit private scheduler/provider details. Blocked delivery waits
at configured cadence and remains pending without consuming a transport attempt.

Stop the owner before historical inspection:

```sh
./target/release/slurm-notify status \
  --config "$HOME/.config/slurm-notify/config.json"
```

Status creates no state, reports busy while monitoring owns it, and labels process
health unknown. Restart preserves checkpoints, frozen IDs/bytes, completed attempts
and pending retries. Delivery is at least once: a crash after provider acceptance
but before local completion can cause a duplicate. Exactly-once/human receipt is
not promised. See [CLI streams/exits](../../contracts/cli/README.md).

## Reconfiguration and upgrades

Changes take effect after restart. Changed endpoints, custom trust or SMTP profiles
must not retarget historical pending work: missing original resources or exact
permissions block it. Review blocked work before changing enrollment; do not delete
the ledger, rewrite frozen payloads or grant broad permissions to force progress.
`init` always creates a new file and enrolls only its displayed current SMTP pairs.

Before replacing a binary, stop monitoring and back up the closed private state
namespace and configuration while preserving permissions. Keep the previous binary
and commit identity. Startup may migrate older supported schema versions; do not
assume an older binary can read a migrated store. Restore a compatible closed
backup for rollback. Historical status refuses migration/repair. No public release
or installation/upgrade support promise is implied by this source-build workflow.
