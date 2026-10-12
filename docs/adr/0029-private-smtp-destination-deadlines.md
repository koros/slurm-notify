# ADR 0029: Private per-destination SMTP deadlines

Status: private cycle adapter; no public contract changes.

## Decision

Add a private selected mixed-cycle entry point accepting an exact SMTP destination
and timeout map. Existing caller-supplied shared-timeout entry points delegate to
the same implementation and retain their behavior. Composed plans supply each
configured SMTP timeout without requiring equal values.

Before clocks, storage, queries or resource lookup, preflight requires at most 64
entries, one unique entry for each current SMTP rendering route, no unknown names,
and positive whole milliseconds up to 30 seconds. Cancellation retains precedence
in the existing cycle. The per-destination path never falls back to a shared
value. Delivery chooses the exact destination's timeout before helper construction
and passes it to the existing bounded transport supervisor and private frame.

Timeouts are operational controls, not durable provider identity. A reopened plan
uses its current deadline for pending work while preserving the frozen message,
rendering/provider binding and exact permission requirement. Changing trust,
authentication identity or credential references still requires matching bindings.

## Evidence and limits

A composed two-destination test gives one child a 40 ms deadline and the other
2,000 ms. Controlled children wait 200 ms before synthetic final acceptance. The
short destination retries while the longer destination completes. After reopen,
raising only the short destination's deadline allows its pending attempt to finish
without changing provider identity or frozen bytes. No ordinary route is selected.

Map validation tests cover zero, sub-millisecond and oversized timeouts, empty,
unknown, duplicate and oversized maps, and boundary-valid milliseconds. Composed
preflight rejects invalid internal deadline state without query, lookup, helper
or outbox writes. Existing shared-timeout, permission, retry and cancellation tests
continue through the common implementation.

Children provide synthetic acknowledgments; no relay receipt, real cluster or
installed activation is claimed. Transport and frame bounds remain unchanged.

## Next

Exercise protected configuration snapshots through private composition and cycles,
including replacement/read failure and reopened pending work. Public contract,
licensing and installed activation gates remain explicit.
