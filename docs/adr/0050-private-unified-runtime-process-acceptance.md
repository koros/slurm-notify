# ADR 0050: Unified private runtime process acceptance

Status: trusted Linux/macOS process acceptance; no installed runtime.

Active lifecycle child fixtures now invoke the unified reported runtime adapter
with explicit request budgets, protected configuration, injected identity/resources
and a tick callback. The adapter supplies the shared cancellation signal used by
composed ticks. Children emit its bounded summary directly, without manually
assembling startup, scheduling or shutdown classification.

Existing acceptance verifies cross-process ownership, atomic source replacement
while a callback is paused, cancellation after committed query/partial send,
metadata-free invalid-restart reporting, unchanged state on failed preparation,
and valid restoration. Fresh child recovery preserves frozen bindings/attempts and
deduplicates events, or sends only remaining ordinary work. State is reacquired and
read-only status succeeds after adapter return.

No real scheduler/provider, OS signals, installed flags, public output guarantees,
dependencies or product defaults are introduced. Hook execution remains cooperative
and caller-bounded; budget exhaustion is not delivery completion.

Next: review and consolidate private foreground readiness and remaining public
activation decisions, using the unified process evidence to identify concrete gaps.
Public contracts, licensing and installed activation remain review gates.
