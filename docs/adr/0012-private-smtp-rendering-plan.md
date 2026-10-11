# ADR 0012: private validated SMTP rendering plans

Status: private test-only experiment; no released route/configuration contract.

A Plan binds explicit SMTP Route inputs to the existing mixed destination list.
Each route supplies a name, sender, recipient, message identity, timestamp and
Hidden/Identifier disclosure choice. There are no defaults or credential fields.
The plan retains immutable borrows of caller inputs, owns a bounded SMTP name
index and exposes no mutable fields. Inputs cannot change while the validated
plan is used. The plan and route/metadata containers omit Debug and Clone.

Construction rejects empty collections, more than 64 destinations or SMTP routes,
and names outside 1..64 ASCII alphanumeric/underscore/hyphen bytes before set
allocation or message parsing. It reuses prepared-poll settings validation to
reject duplicate/missing bindings and invalid/overflowing message counts. SMTP
routes must be a subset of destinations; non-SMTP destinations require no SMTP
metadata. Explicit Message-ID tokens must be distinct across SMTP routes.

A local bounded message-builder probe validates each route's envelope, message ID
and frozen timestamp before any scheduler query, clock sample, SQLite write or
provider call. The probe builds and discards a fixed empty-body message, using the
same metadata checks as actual rendering rather than duplicating validation rules.
It does not persist or send a probe message. This is synchronous bounded work,
not a production verification of relay addresses or credentials.

Plan::poll_with composes the existing atomic prepared poll using exactly the
validated bindings and disclosure choices. Event-dependent validation remains in
the renderer: a scheduler's numeric ID may exceed its 32-byte disclosure bound.
A later event/route failure discards the complete candidate before SQL writes;
Hidden rendering can recover such an event after reopen. Unknown SMTP route
lookups and renderer errors expose static categories without caller values.

Four tests cover invalid envelope/ID/date metadata, malformed/duplicate/missing
route bindings, reused Message-ID tokens, zero/overflowing message bounds,
collection/name limits and their valid 64-route/64-byte boundaries. They verify
explicit per-route disclosure, unknown-route/event rejection, oversized disclosed
job-ID failure without durable state, reopen recovery with hidden IDs and replay
preserving original frozen bytes despite a subsequent disclosure change.

This is a separate private polling composition; the prepared orchestration tick
still accepts arbitrary fixture preparation hooks. Next: connect validated plans
to that tick while retaining cancellation-first behavior, preflight before side
effects, finite budgets and maintenance after event-dependent preparation failure.

Static caller-supplied fixture IDs/timestamps are deliberate experimental inputs.
Per-event production Message-ID/domain/timestamp selection, durable route revision,
credentials, privacy defaults and public review remain unresolved. No schema,
dependencies, public interfaces or executable defaults change.
