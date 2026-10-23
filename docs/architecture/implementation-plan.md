# Current implementation plan

The private engine and OS resource/signal lifecycle are implemented and tested.
The installed executable supports the approved help/version slice. [Product readiness](product-readiness.md)
contains current capability evidence and limitations; [implementation history](implementation-history.md)
preserves past increments without treating their next steps as current instructions.

## Current MVP milestone

The target is a local foreground MVP with generic HTTPS webhook and required
STARTTLS SMTP delivery, explicit JSON configuration, durable recovery, and
setup/test/doctor/status workflows. Broader local MVP contract proposals were adopted on 2026-10-13;
help/version, webhook/SMTP configuration validation and historical status are installed. MIT licensing, refreshed guidance,
90% coverage and accurate badges are complete.

Continuous scheduling and first-signal reason retention are now implemented
privately ([ADR 0058](../adr/0058-private-continuous-scheduler-and-signal-reason.md)).
Continuous scheduling now retains one owned session across poll-batch renewal
([ADR 0059](../adr/0059-private-owned-continuous-session.md)). Historical status is
installed ([ADR 0062](../adr/0062-installed-historical-status.md)); next complete
mixed public-event/SMTP composition and bind OS-backed orchestration and monitoring commands.

## Next product milestones

1. Completed: the approved [help/version slice](../proposals/finite-cli-activation-v1.md)
   has exact output/exit snapshots and installed-binary acceptance.
2. Completed: installed webhook/SMTP configuration validation and offline historical
   status, with deliberate schemas/output baselines and no hidden sending.
3. Transactional webhook public IDs/bytes/route revisions are implemented and tested
   ([ADR 0063](../adr/0063-transactional-public-webhook-events.md)). Mixed SMTP/public storage preparation now shares that transaction. Next wire
   composition admission and bind stored frames to supervised delivery.
4. Map retained signal reasons and bind OS-backed continuous orchestration; complete
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
