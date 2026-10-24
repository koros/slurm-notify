# Current implementation plan

The installed executable supports help/version, configuration validation, historical
status and private bounded webhook/SMTP child protocols. Installed foreground
monitoring remains unavailable. [Product readiness](product-readiness.md) records
current evidence and limits; [implementation history](implementation-history.md)
preserves historical increments.

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
installed ([ADR 0062](../adr/0062-installed-historical-status.md)). Public mixed
composition and OS-backed orchestration are tested privately; installed helper
binding and monitoring activation remain pending.

## Next product milestones

1. Completed: the approved [help/version slice](../proposals/finite-cli-activation-v1.md)
   has exact output/exit snapshots and installed-binary acceptance.
2. Completed: installed webhook/SMTP configuration validation and offline historical
   status, with deliberate schemas/output baselines and no hidden sending.
3. Public UUIDs, frozen payloads/route revisions and mixed SMTP work commit atomically
   ([ADR 0063](../adr/0063-transactional-public-webhook-events.md)). Guarded mixed
   delivery, runtime cycles and owned continuous orchestration have private recovery
   acceptance. Same-executable helper selection and installed bounded child handlers
   exist ([ADR 0064](../adr/0064-private-same-executable-provider-selection.md)).
   Installed SMTP helpers have verified STARTTLS/AUTH and identical-byte replay after
   a temporary final reply. Successful webhook helper send and installed monitoring/
   durable-store restart acceptance remain pending.
4. Complete installed orchestration/helper binding and
   webhook-only composition and status contention acceptance, then activate reviewed
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
