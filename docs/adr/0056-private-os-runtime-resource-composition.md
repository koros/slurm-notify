# 0056: Private OS resources composed with unified runtime

Status: accepted for private Linux/macOS process fixtures only.

Trusted children invoke unified protected startup with the actual effective-UID and
explicit environment adapters. Each child has a cleared environment plus named
synthetic webhook resources and invalid USER/LOGNAME values. Accounting callbacks
verify the effective-user query argument and return synthetic records for that user.
No Slurm subprocess or provider request runs. SMTP permission remains absent; the
helper/ordinary-send callbacks must never execute.

Parent supervision caps output at 96 accepted bytes, uses a two-second child deadline,
and kills/reaps/joins on assertion failure. Exact static preparation/ownership or
exhaustion reports reveal no account names, resource values or source diagnostics.
The deadline bounds this fixture process, not production NSS/environment lookup.

Missing, empty, oversized, non-Unicode and invalid URL/header values fail before
creating absent state. Valid resources commit one frozen blocked SMTP event. Repeat
all invalid-resource cases with existing state and verify unchanged file content.
A separate owner blocks valid startup without writes. After release and restoration
of valid resources, replay creates zero events and preserves frozen message, provider
token, attempt and checkpoint. SMTP credentials are not required for denied work.

OS adapters stay test-only. No new dependencies, defaults or public schemas/commands
are introduced. Host account lookup can use configured NSS services; no support
matrix or real-cluster acceptance is inferred. Next combine these OS resource hooks
with signal-driven foreground lifecycle and durable recovery acceptance.
