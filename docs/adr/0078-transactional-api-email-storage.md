# ADR 0078: transactional API-email frames and scoped pauses

Status: adopted for private foundation implementation, 2026-10-27.

The next foundation increment adds persistence primitives before normal API-email
monitoring is enabled. Existing monitoring config types, permission enrollment,
CLI recovery and OAuth refresh are unchanged and remain separate integration work.

## Frozen request transaction

Extend the private selected public route with an optional validated API-email
profile. A route cannot request both Slack detail and API-email rendering. After
assigning the existing canonical UUID, derive minimal email content and deterministic
provider body/content type inside the same transaction as routes, event frames,
checkpoint, ledger and outbox. Preparation/budget/migration/commit failure rolls
back all of them. No extra UUID, altered webhook bytes or separate late preparation.

The first selected API-email transaction upgrades schema 4/5/6 through the existing
public/Slack migrations and additive migration 0007. Merely opening state or running
`email test` does not activate migration 0007. The email_event_frames table contains
only route identity, secret-free profile reference, content type, exact request body
and SHA-256 integrity over the content type and body. Never store credentials,
OAuth tokens or raw responses. Foreign keys and sealing/update/delete triggers
make the bytes immutable. Existing v4/v5/v6 state remains readable; v7 is now a
known schema for historical inspection. Old binaries reject v7.

Loading checks the exact public route revision, canonical frame integrity, current
profile metadata reference, frozen wire integrity and configured byte limit before
returning bytes. Changed recipient/account/provider/domain/resource references fail
closed. A later adapter must additionally bind owned trust snapshots and enrolled
API-email permissions before credential lookup. A profile-reference fingerprint
alone does not authorize a send.

## Provider pauses

Schema 0007 also defines email_provider_pauses, scoped by cluster and destination,
with exact resource revision, static reason (authentication, policy, credentials)
and bounded Unix timestamp. No provider error text or account address is retained.
The preparation and recovery methods are tested privately; no public resume command
or normal monitoring API-email type is enabled by this increment.

A preflight pause requires a matching pending API-email route and records no send
attempt. A post-submission pause records a transient attempt, its next due time and
the pause in one transaction. Callback/storage/revision failure rolls back both.
The existing record_delivery path uses the same transaction helper with no pause,
preserving legacy behavior. The caller remains responsible for typed submission
stage accounting; OAuth/userinfo exchanges must not count as email requests.

Due-delivery selection excludes paused destinations before applying the batch limit,
so an affected provider cannot consume every slot or block unrelated routes. Pauses
survive restart. An explicit resume primitive validates exact revision and pending
route compatibility, then removes only that pause. It never clears attempts,
rewrites messages or makes exhausted deliveries eligible for an extra send. Public
CLI authorization and exhausted-item requeue remain a separate contract increment.

## Retention and upgrades

Email request bodies contain sender/recipient and disclosed job content. Like other
immutable public/Slack frames they live with the event ledger, not merely until
outbox cleanup. Protected state/backups therefore retain that metadata for the
ledger lifetime. Permission/setup integration must disclose this before activation.

Before enabling a future API-email monitoring destination, drain older pending
work, stop the owner and take a closed config/state backup. Downgrade from schema
v7 requires restoring that backup; accepted later messages can replay. Never copy
live SQLite files, delete state or change user_version to force a downgrade.

Controlled tests cover three provider encodings, immutable content type/body,
changed-profile rejection, exact reopen/replay, preparation/late migration rollback,
preflight/post-submission pause accounting, unrelated-route progress, restart and
resume without attempt resets. No real provider, Slurm access or supported rollout
is established by these foundation tests.
