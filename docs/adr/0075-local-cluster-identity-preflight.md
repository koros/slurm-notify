# ADR 0075: local Slurm cluster identity checks

Status: adopted, 2026-10-25.

The operator resolved a first-poll storage failure by changing the configured
hostname-like label to Slurm's actual ClusterName. cluster_id is a scheduler
identity, not a freely chosen display label. Do not normalize accounting records to
an arbitrary configured label or rewrite an established namespace automatically:
that could merge clusters or break deduplication.

Doctor and installed start optionally discover `scontrol` beside the canonical
`squeue` executable. If absent, retain compatibility with installations offering
only squeue/sacct. If present, require it to be executable, canonicalize it and run
`show config` with argument arrays, a query timeout and bounded output. Discard raw
stdout after extracting exactly one bounded ClusterName; stderr stays private.
Clear SLURM_CLUSTERS for this local probe, retain the normal configured Slurm site
environment, C locale and UTC. Accounting stores cluster names in lower case, so
compare the lower-case discovered name with the explicit configuration.

Doctor shares its existing total probe deadline and remaining output budget with
this optional probe. Start gives it the configured query budget before acquiring
state. Probe failure exits 1; mismatch exits 2 with actionable static guidance and
no monitoring state opened. No job data or provider traffic is queried. There is no
new hard dependency on scontrol, configuration default or network provider behavior.
A doctor success on a site without the sibling probe does not prove cluster identity.

Every scheduler poll also checks current-user accounting cluster identity before
reconciliation and persistence. A mismatch is a typed error that stops monitoring
with `stage=poll reason=cluster_identity` and a correction hint. It does not become
a misleading storage failure or endless retry. No candidate/checkpoint/outbox is
committed for the rejected poll. Empty accounting output cannot establish identity;
this fallback checks it when records become available.

The setup guide discovers the cluster rather than hard-coding a host label. Tests
cover missing/invalid probes, case normalization, mismatches, output bounds and the
installed no-state-opened guarantee. The operator's existing correctly configured
state remains intact; no automatic rename or migration is performed.
