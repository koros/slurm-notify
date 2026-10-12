# First installed CLI activation proposal

Status: proposed for explicit review. No installed behavior or approved baseline
changes in this document. Approval applies only to the grammar/outputs below;
CLI/config/SMTP/payload/limits drafts remain separate review decisions.

## Recommended first slice

Activate help/version before configuration commands or monitoring. This provides
an installed-binary compatibility baseline without depending on unresolved public
payloads, foreground exit reasons, scheduling renewal or configuration schemas.
The full local CLI proposal remains the intended later scope; SMTP remains required
for the MVP. This changes the recommended implementation order, not MVP scope.

## Exact grammar and snapshots proposed for approval

Accept exactly one argument: `--help` or `--version`. No short aliases, positional
commands, combined options, implicit configuration path or environment defaults.
Interpret OS arguments without lossy conversion. Duplicate, conflicting, unknown
and non-Unicode arguments are invalid. Help/version read no config, state,
credentials or account database; start no worker, Slurm process or provider request.

`slurm-notify --help`: stdout exactly the following UTF-8 bytes, with a final newline;
stderr empty; exit 0:

```text
Usage: slurm-notify --help | --version

Options:
  --help     Show this help
  --version  Show the package version

Monitoring commands are not available in this build.
```

`slurm-notify --version`: stdout `slurm-notify <package-version>\n`, stderr empty,
exit 0. Substitute Cargo's package version exactly, independent of config/event
schema versions. For the current unreleased package the snapshot is:

```text
slurm-notify 0.0.0
```

No arguments: stdout empty, stderr exactly `Usage: slurm-notify --help | --version\n`,
exit 2. Every other argument sequence: stdout empty, stderr exactly
`Invalid invocation. Use --help for usage.\n`, exit 2. Do not echo rejected arguments,
paths, environment values or parser error detail. Output has no ANSI sequences.

## Acceptance after explicit review

Record approval scope and any edits before creating approved CLI snapshots under
`contracts/cli/`. Test exact bytes/streams/exits through the installed executable:
help, version, no arguments, unknown commands/options, duplicates, conflicting
flags, positional values and non-Unicode arguments on supported Unix hosts.
Verify cleared/invalid environment and inaccessible or absent config/state cause
no different output or creation. Update the placeholder smoke expectation and
shared checks deliberately; do not regenerate snapshots to hide changes.

The existing placeholder exits 1. This proposal deliberately changes only these
invocations to the specified exits and outputs; it creates no monitoring capability.
No new parser dependency is required for this two-option grammar. Later additions
need their own reviewed snapshots and version/compatibility assessment. Dependency notices,
product support and release authorization are independent of this CLI review.
