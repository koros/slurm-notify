# ADR 0065: Installed SMTP/mixed foreground command

Date: 2026-10-24

Status: implemented prototype activation; real-cluster/provider and release acceptance remain pending.

## Scope and compatibility

Activate the adopted `start --foreground --config <absolute-path>` grammar on Linux
and macOS. Options may surround the command; duplicates, unknown words, extra values,
relative/non-Unicode paths and omitted `--foreground` fail before resource access.
`start --help` is resource-free. Global help intentionally adds the command; version
and finite command outputs retain their existing baselines. No service is installed.

This increment requires at least one SMTP provider. SMTP-only and mixed HTTPS
webhook/SMTP configuration are accepted. Webhook-only monitoring exits 2 before
state creation; its owned-runtime composition remains a separate activation gap.
Provider helpers use ADR 0064's same-executable versioned commands and bounded stdin.
Successful installed SMTP delivery/restart is accepted using synthetic scheduler
programs and an ephemeral verified-STARTTLS loopback relay. This is not real-provider
acceptance or a supported monitoring release. Successful webhook helper sending
still needs acceptance. Setup, test and doctor commands remain unavailable.

## Resource and scheduling choices

- Read one protected configuration snapshot and retain its prepared trust, routing,
  resources and operational controls for the owned run. SMTP credentials are resolved
  only after exact frozen profile/rendering permission checks. A blocked destination
  remains pending and reports a static diagnostic; it is not silently retired.
- Resolve `squeue` and `sacct` once from PATH: at most 4,096 encoded bytes and 64
  absolute, nonempty directory entries. Choose the first regular file with an
  executable mode bit, canonicalize its pathname, and pass argument arrays directly.
  No capability probe, shell invocation or provider test occurs during discovery.
  Filesystem/identity lookup is synchronous; metadata discovery does not guarantee
  later ACL access, executable availability or site policy permission.
- Reuse configured `monitor.query_timeout_seconds` for SQLite busy waits. Keep the
  configured scheduler/output and per-provider deadlines. An internal batch of 64
  polls renews within the same owned session, retaining health; it does not cap the
  run or change configured completion-relative poll cadence. Blocked delivery waits
  that cadence rather than spinning. No new user configuration defaults are added.
- Honor `maintenance.smtp_revision_cleanup_limit` as a shared per-cycle ceiling
  across unbound provider/rendering descriptors; zero disables pruning. References
  protect pending work and retained evidence. Retention and health use existing
  configured bounds. Health counts are explicitly labelled samples with truncation.

Canonical scheduler/helper pathnames are retained, not inode-pinned. Executable
replacement and synchronous resource lookup limits remain operational constraints.
Validate site polling/execution policy before running against a real cluster.

## Failure and output semantics

Configuration/resources and missing scheduler executables exit 2. Unsafe, busy,
unsupported or unrecoverable state exits 1. Corrupt checkpoint/storage, invalid
runtime time/policy, cleanup or health failures stop the run with exit 1. Transient
scheduler and delivery failures report static diagnostics and retry at configured
cadence; known outcomes commit before later failure. Finite `config validate`
continues to return 3 for denied current SMTP pairs; monitoring instead keeps blocked
work pending so configuration/profile repair can be followed by restart.

SIGINT/SIGTERM stop new work and return 130/143 using the first retained signal;
active work remains bounded by parent transport/query deadlines. Late signals do not
replace an existing failure. The owner and signal binding are released on every exit.

Startup/shutdown/blocked diagnostics go to stderr. Labelled sampled health goes to
stdout. No raw job rows, URLs, credentials, headers, message bodies or private paths
are printed. Public start diagnostics are human-readable; private fixture summary
strings and internal tick budgets are not exposed as a product protocol.

## Acceptance

Binary tests cover explicit grammar and help, missing resources before state creation,
webhook-only rejection, busy ownership, blocked SMTP without credential lookup,
SIGINT/SIGTERM exits, frozen bytes across fresh-process restart, sampled health and
fatal corrupt checkpoint handling. A configured loopback TLS relay verifies actual
SMTP delivery through the selected child; SQLite records one delivered attempt and
a restarted monitor does not replay it. Ordinary tests use no real Slurm cluster or
external provider. Shared checks and the 90% coverage gate remain mandatory.
