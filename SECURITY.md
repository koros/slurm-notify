# Security policy

Slurm Notify is an unreleased prototype. No version has a supported
security-maintenance or deployment guarantee, and no response-time SLA is promised.
Synthetic tests do not authorize monitoring a cluster or using a provider account.

## Report a vulnerability privately

[Open a GitHub private vulnerability report](https://github.com/koros/slurm-notify/security/advisories/new).
Private vulnerability reporting is enabled for this repository. Include the commit,
a synthetic reproduction, expected/actual behavior and the affected boundary.
If GitHub reporting is unavailable, contact korosmatick@gmail.com privately.

Do not post credentials, authorization headers, secret webhook URLs, private paths,
real scheduler output, job metadata or state databases in public issues. Do not
exercise suspected issues against clusters or accounts without authorization.
Conduct reports belong to the [Code of Conduct](CODE_OF_CONDUCT.md) contact.

## Credential and data handling

- Provider secrets are read from named environment variables. Inline config secrets
  and credentials in command arguments are not supported. Environment access by
  other processes is still subject to the host's user/process security model.
- Config, trust and state resources must be protected and user-owned; config files
  use mode 0600 and private setup directories use mode 0700. Do not share state or
  backups. Opted-in Slack job names remain in the immutable ledger and backups even
  after delivery-retention cleanup.
- SMTP requires verified STARTTLS, explicit protected DER trust roots and reviewed
  sender/recipient permissions. It does not fall back to system trust roots.
  HTTPS uses bundled public roots unless an explicit DER bundle replaces them;
  certificate/hostname verification remains required. The client uses direct
  connections rather than proxy environment variables.
- Current-user scheduler filtering, bounded subprocesses and supervised provider
  processes limit exposure. They do not provide isolation from a compromised user
  account, scheduler or destination provider.

## Operational limits

A lost acknowledgement can cause duplicate delivery; durable retries do not mean
exactly-once receipt. Historical status does not prove a monitor is running.
Startup filesystem/environment/NSS operations do not have a universal hard deadline.
A live SQLite file must not be copied as a backup; stop all owners first and follow
upgrade guidance. Check site polling/execution policy before real-cluster use.

Dependency advisories and licenses are checked with cargo-deny; these checks are
not a security audit. Report failures of the documented boundaries privately.
Supported targets, release artifacts, dependency/threat review and operational
acceptance remain [release gates](docs/operations/support-and-acceptance.md).
