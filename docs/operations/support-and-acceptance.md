# Prototype support and acceptance

The local MVP command set is implemented: init, config validate, doctor, explicit
provider test, foreground start, historical status, help and version. The product
remains an unreleased prototype; there is no supported deployment matrix or public
Rust API. Source builds do not authorize real-cluster tests, external notifications,
service installation or release publication by an agent.

## Evidence that exists

- Synthetic scheduler parsing and bounded process/resource execution.
- Protected resources, exact SMTP permissions and owned durable state.
- Controlled verified HTTPS and required STARTTLS SMTP delivery, failures and retry.
- A relocated executable completing terminal setup, validation, doctor, mixed
  delivery, partial completion, frozen HTTPS replay and SMTP no-replay restart.
- Shared formatting/strict Clippy/locked Rust/process/Python checks, hooks and a
  90% Rust line coverage gate. Current local results are in
  [product readiness](../architecture/product-readiness.md).

CI is configured for Ubuntu 22.04 and macOS 14 with the pinned development compiler.
That is a validation matrix, not an MSRV, libc, Slurm or product-support commitment.
Windows, remote SSH, log monitoring, implicit SMTP TLS, extra authentication modes,
Slack/Teams-specific contracts and background services are outside this local MVP.

## External acceptance before a supported release

1. Obtain an explicitly authorized cluster/site and its permitted polling/execution
   policy. Agree a Slurm/version/accounting/timezone/job-identity baseline. Check
   current-user filtering, terminal states, arrays/requeues, accounting delay,
   restart/catch-up behavior and source-data privacy without retaining raw private
   diagnostics in fixtures or public reports.
2. Obtain authorized intended webhook/SMTP destinations and exercise verified trust,
   credentials, temporary/permanent refusal, unknown acceptance and recovery. Do not
   send test notifications to third parties without authorization.
3. Verify source installation, filesystem semantics, termination, migration, closed
   backup/rollback and permissions on each intended OS/architecture/filesystem.
   Set and document compiler/libc/Slurm support baselines from that evidence.
4. Complete maintainer review of interface/migration compatibility, dependency
   license texts/notices, threat model, vulnerability reporting and repository
   review/check protection. Review unknown acceptance/duplicate behavior with users.
5. With explicit release authorization, prepare target binaries, checksums,
   provenance, dependency notices and upgrade notes; verify installation from those
   exact artifacts before publishing. No release has been published by this work.

The repository's MIT license covers project code; dependencies keep their licenses.
Use [the security policy](../../SECURITY.md) for reporting and keep private resources
out of public issues. Existing local acceptance cannot substitute for site/provider
permission or operator review of a release.
