# ADR 0068: Installed read-only doctor

Date: 2026-10-24
Status: implemented.

`doctor --config <absolute-path>` accepts the finite command grammar used by status,
including options before the command. Help is resource-free. Read one protected
snapshot and validate all configured resources/exact SMTP permissions. Run only
`squeue --version` and `sacct --version`, discovered through the same bounded
absolute PATH policy as start. No provider request, job query, state creation,
installation or enrollment occurs. State health/ownership remains the explicit
historical status command and is labelled uninspected by doctor.

Share configured query timeout across both probes. Reserve each process's bounded
stderr allowance and actual stdout bytes from a shared configured output budget;
stdout/stderr remain individually bounded by the runner. Reject empty/non-ASCII
probe output, unsuccessful execution, timeout and excessive output. Never print raw
version or scheduler diagnostics. Success proves executable probing only, not Slurm
compatibility, accounting completeness or site polling permission.

Exit 0 uses doctor-success.stdout, 2 means invalid config/resources or missing
executables, 3 means exact SMTP permission denial, and 1 means failed probes.
Global help adds doctor deliberately. Existing command outputs remain stable. No
configuration/dependency/default/storage changes accompany this command.

Installed acceptance covers argument/help grammar, success, missing executables,
nonzero/empty/over-budget/stalled probes and unsafe config permissions, and verifies
no state or loopback provider request. Existing resource permission tests remain.
