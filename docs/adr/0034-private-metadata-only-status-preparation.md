# ADR 0034: Private metadata-only status preparation

Status: private adapter; no installed status/configuration contract.

## Decision

Prepare a separate status plan from bounded bytes or one protected configuration
snapshot. Full draft shape, reference and control validation remains mandatory.
Retain only owned zeroizing cluster/storage metadata and the identity/event limits,
stale threshold, retention and health sample bound needed for reporting. Discard the
provider tree, resource references, permission tuples and unrelated controls after
validation. The plan has no Debug/Clone implementation.

Preparation offers no identity, environment, trust or transport callbacks and makes
no provider-resource access. A structurally valid configuration can therefore report
state even when trust files or referenced resources are unavailable. This establishes
status metadata readiness, not provider/authentication/delivery readiness.

Status uses the existing coherent bounded health/checkpoint reader against an
explicit caller-owned Store and supplied cycle/time. Preparing metadata does not
create or acquire state. Storage metadata is available to a future state owner but
is not silently interpreted as permission to initialize or repair a database.
No default path, hot reload, public command or output format is introduced.

## Evidence and limits

Three tests cover owned metadata surviving source-byte destruction with unavailable
provider references and absent trust; full provider/route validation and protected
missing/malformed/unsafe-file rejection; and protected preparation followed by owned
state reopen, fresh/stale status and event-only capacity pressure. An invalid atomic
file replacement does not alter an already prepared plan, and later preparation
rejects it. Pending event identities/attempt numbers remain unchanged. Debug reports
exclude identifying and resource metadata. No real environment reads or network
access occur, and preparation leaves the configured storage path absent.

State acquisition remains caller driven. Status access to an absent state namespace
or one owned by a separate running process has not been composed into an installed
flow. Reopened cycle failure counters remain ephemeral. Filesystem support remains
limited to the protected reader's existing platform policy.

## Next

Add private health-only inspection of existing state without initializing or repairing
it, with absent/unsafe state and concurrent-owner acceptance. Public contracts,
licensing and installed activation remain review gates.
