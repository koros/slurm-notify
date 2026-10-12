# Current implementation plan

The private engine and OS resource/signal lifecycle are implemented and tested.
The installed executable remains a placeholder. [Product readiness](product-readiness.md)
contains current capability evidence and limitations; [implementation history](implementation-history.md)
preserves past increments without treating their next steps as current instructions.

## Current maintenance milestone

MIT licensing is in place, current documentation/agent guidance is refreshed, and
the shared coverage script and CI enforce 90% Rust line coverage. README badges
reflect current status; the build artifact and Cargo metadata include MIT licensing.

## Next product milestones

1. Implement the reviewed first installed CLI slice: help/version with exact output,
   invocation errors, stream/exit behavior and installed-binary acceptance. The
   [proposal](../proposals/finite-cli-activation-v1.md) remains pending review.
2. Settle config/status semantics and baselines; connect existing protected Rust
   preparation and OS resources to installed commands, with no hidden sending.
3. Complete public event persistence: opaque IDs, frozen webhook bytes, immutable
   routes and source timestamp/null semantics. Assess the UUID source/dependencies.
4. Complete continuous foreground scheduling, signal-specific exit reasons,
   webhook-only composition and status contention behavior, then activate reviewed
   webhook/SMTP flows with ownership, privacy and restart acceptance.
5. Implement setup/test/doctor UX; validate an authorized cluster/provider and agreed
   platform/Slurm baseline. Complete security reporting, installation/upgrade,
   dependency notices and explicitly authorized release acceptance.

Slack/Teams scope remains undecided. Log analysis and remote SSH follow the local
MVP. No supported monitoring release or public Rust library API exists yet.

## Validation

`sh scripts/check.sh` is the shared local/CI suite; hooks must pass.
`sh scripts/check-coverage.sh` enforces at least 90% aggregate Rust line coverage.
Current recorded results belong in product readiness, not duplicated historical
status paragraphs in this plan or README. Hosted CI status must be verified separately.
