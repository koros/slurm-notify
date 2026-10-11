# Local CLI v1 proposal

Status: draft for explicit review. No command, flag, exit code or output below is
implemented or approved. The installed executable continues to exit with status 1.

## Scope and grammar

Propose local foreground monitoring for one configured cluster and the effective
user. Do not introduce a supported Rust library API. Global `--help` and `--version`
require no configuration or side effects. Command help also requires no config.
Accept options before or after the command; reject duplicates and unknown options.
Positional provider names follow the configuration alias rules. Diagnostics go to
stderr, requested results to stdout. Plain text is the initial output format;
machine-readable output is deferred rather than exposed without its own schema.

| Invocation | Proposed behavior |
| --- | --- |
| `slurm-notify --help` | Print available commands and usage; exit 0 |
| `slurm-notify --version` | Print `slurm-notify <package-version>` and a newline; exit 0 |
| `slurm-notify init --config <absolute-path>` | Interactive setup; show providers, transmitted fields and selected policy; confirm before atomically writing a new config; do not send or start monitoring |
| `slurm-notify config validate --config <absolute-path>` | Parse, validate structure and referenced local resources; report missing values without printing them; perform no scheduler queries or network requests |
| `slurm-notify doctor --config <absolute-path>` | Check config, local permissions and availability of configured Slurm executables; run bounded capability probes only as described below; send no provider request |
| `slurm-notify test <provider-name> --config <absolute-path>` | Explicit authorization for one synthetic test alert to that destination; show its safe identity and synthetic fields; perform no scheduler poll or persistent monitoring |
| `slurm-notify start --foreground --config <absolute-path>` | Validate configuration and own state before polling; run until cancellation or a fatal failure; never install a service |
| `slurm-notify status --config <absolute-path>` | Inspect an existing store read-only; report durable queue/poll state and its age; create no state and infer no running-process status from a checkpoint |

No-argument invocation prints short usage to stderr and exits 2. An absolute config
path is required for every configured command; there is no implicit search path.
`--foreground` is required so persistence expectations are visible. No background,
stop, logs, remote-mode or service-install command is proposed in this slice.
`providers list`, noninteractive setup and structured output follow separate drafts.

`init` requires a TTY; without one, exit 2 and identify the unsupported operation.
Refuse to overwrite an existing file. Create new config with user-only permissions,
reject symlink paths, write in the same directory and rename atomically. Missing
parent directories need explicit setup handling; do not create an arbitrary tree.
An eventual noninteractive setup mode must not silently inherit wizard choices.

## Exit and failure policy

| Exit | Meaning |
| --- | --- |
| 0 | Requested finite command succeeded; provider test was accepted by the relay/service, not proof of human receipt |
| 1 | Runtime, storage, scheduler or transport failure prevented the requested operation |
| 2 | Invalid invocation/configuration, unavailable required resource or unsupported operation |
| 3 | Valid input blocked by an explicit delivery permission/profile policy |
| 130 / 143 | Foreground run stopped by SIGINT / SIGTERM respectively on supported Unix systems |

An SMTP credential or trust resolution error is config/resource failure (2) for
finite commands. Permission denial or a pending-profile mismatch is policy failure
(3). During monitoring, transient query/delivery failures update visible health and
retry within configured budgets. A blocked destination remains pending and is
reported; the foreground loop may continue polling but must not spin on the blocked
row. Exact blocked-batch scheduling needs an acceptance test before activation.
Corrupt state, lost ownership or unsafe state permissions stop the foreground run
with exit 1. An exhausted destination remains terminally failed and visible.

Cancellation stops new polls/sends, finishes bounded active work and commits known
outcomes. Do not promise immediate interruption of resolver callbacks or network
work; parent transport deadlines apply. A supervisor crash can leave acceptance
unknown and permit replay. Signals are not a portable Windows contract in v1.

## Output and privacy

Define semantic fields now; freeze exact text in reviewed CLI snapshots before
activation. Successful validation reports the config version, cluster alias,
destination count and permission status. Status reports checkpoint age, queued,
retrying and terminally failed counts, and known storage/poll problems. Historical
durable state is labelled historical; it cannot prove current process health.
No raw scheduler output, credentials, secret URLs, authentication headers, message
bodies or private filesystem paths are included in normal output.

Safe provider identity is its alias and type. Setup/test may display HTTPS scheme
and host without path/query/userinfo, or SMTP relay/port/TLS mode and verification
name. Show the exact synthetic fields before setup approval; `test` explicitly
requests sending and needs no second interactive confirmation. Use deterministic
plain output without ANSI color or terminal control sequences initially.

Doctor capability probes use argument arrays, configured total query/output limits,
effective-user filtering and no job submission/cancellation. Executable discovery
does not prove cluster permission or accounting completeness. Doctor reports that
site polling/execution policy must be validated before start; no fabricated
"cluster compliant" success is allowed.

## Acceptance before activation

Snapshot help, version, usage, finite success/error outputs and exit codes. Test
unknown/duplicate flags, missing/relative paths, no TTY, failed atomic writes,
symlinks, unavailable commands, redaction, read-only status and no hidden network
requests. Test explicit provider sends, foreground cancellation, blocked work,
partial progress/restart and loss of state ownership through installed binaries.
Approve any later command, output/default or exit-code change with compatibility
assessment. This draft introduces no installed commands or public snapshots.
