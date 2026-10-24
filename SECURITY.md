# Security policy

Slurm Notify is an unreleased prototype. No version currently has a supported
security-maintenance or deployment guarantee. Do not treat passing synthetic tests
as approval to monitor a cluster or send to a provider account.

## Reporting

Use GitHub private vulnerability reporting on this repository's Security tab when
available. If that channel is unavailable, ask the maintainer (`koros`) for a
private reporting channel without posting vulnerability details or sensitive data
in the public request. No response-time SLA is promised.

Never include credentials, authorization headers, secret webhook URLs, private
paths, real scheduler output, job metadata or state databases in a public issue.
Use synthetic examples and sanitized fixed diagnostic categories. Do not test a
suspected issue against clusters or provider accounts without authorization.

## Boundaries

The local prototype uses protected user-owned config/trust/state, current-user
scheduler filtering, verified TLS, exact SMTP profile permissions and supervised
provider children. Delivery can be duplicated after unknown acceptance. Startup
filesystem/environment/NSS operations do not have a universal hard deadline.

Report failures of these boundaries privately. Supported targets, dependency/threat
review, release artifacts and operational acceptance remain release gates described
in [support and acceptance](docs/operations/support-and-acceptance.md).
