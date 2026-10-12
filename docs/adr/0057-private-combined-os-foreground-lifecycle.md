# 0057: Private combined OS foreground lifecycle acceptance

Status: accepted for private Linux/macOS process fixtures only.

Combine effective-UID identity, explicit named environment resources and the
SIGINT/SIGTERM bridge with unified protected runtime orchestration. Trusted children
start with a cleared environment and synthetic resource values; USER/LOGNAME remain
invalid. Bind before startup, commit one blocked tick, emit READY and enter real
foreground scheduling. Synthetic accounting and denied SMTP still prevent external
Slurm execution or provider sending.

For each signal, the parent verifies exclusive ownership while READY, then signals
its owned PID. The child returns the exact bounded one-tick cancellation summary,
drops/joins the bridge and verifies fresh rebinding. Parent deadlines/reaping cover
startup and shutdown; stdout accepts only six READY bytes plus a capped 96-byte
summary. Child ownership is guarded before its output-reader thread starts.

Compare existing state files before/after shutdown and verify frozen bytes, provider
token, attempt and checkpoint. A fresh valid-resource replay creates no event and
preserves that pending work; status reacquires afterwards. Existing invalid-resource
and owner-contention startup cases remain in the same acceptance package.

This closes private composition evidence for these OS hooks at the blocked-tick
boundary. It does not activate installed commands or prove real-cluster/provider
compatibility. NSS/lookup duration, inherited blocked signals and competing handlers
retain ADR 0053/0056 limitations. Public contracts, payload transactions, onboarding,
support/license and release gates remain. Next consolidate the concrete installed
activation review from this combined evidence before choosing an activation slice.
