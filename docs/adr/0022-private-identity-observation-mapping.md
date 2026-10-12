# ADR 0022: Private identity and observation mapping

Status: private experiment; no approved user lookup or configuration contract.

## Decision

Add test-only `observation_mapping` to map explicit cluster/user-only/initial
observation controls and an injected scheduler-user lookup into owned metadata.
Require user-only monitoring and a 1..64 ASCII alphanumeric/underscore/hyphen
cluster alias before lookup. Accept only explicit `emit` or `suppress` choices;
there is no default. Request at most 256 user bytes, then recheck nonempty bounded
printable ASCII without whitespace, pipe separators or a leading flag prefix.

Retain cluster and scheduler user in zeroizing owners without Debug/Clone, and
expose borrowed validated metadata and the existing emit_discovered boolean.
Errors are static policy/lookup/identity categories without source text.
No dependencies, ambient lookup, scheduler command or state creation occurs in
construction.

## Evidence and limits

Three tests cover policy rejection before lookup, owned identity after source
removal, requested/max accepted bounds and static invalid-source failures. Durable
poll acceptance covers both initial choices, current-user command filters, exclusion
of another user's fixture job and checkpoint/deduplication persistence. Reopening
and selecting the opposite initial policy does not replay an already recorded or
suppressed terminal event. Fixtures use temporary SQLite and synthetic query output;
no Slurm or external network access is required.

The injected source is responsible for mapping effective process ownership to the
site's scheduler identity, allocation and deadline policy. This module does not
verify that association; it must not be claimed as effective-user authorization.
The private ASCII/256-byte user rule needs support/site review. No portable OS/NSS,
remote user or username override adapter is introduced. Initial suppression remains
the existing durable ledger behavior; no public state/config semantics are frozen.

## Next

Prepare private lifecycle-event routing selection before public configuration
mapping. Installed lookup, parser, commands and activation still require explicit
contract review and cluster-policy validation where applicable.
