# ADR 0028: Private composed mixed cycles

Status: private acceptance adapter; no installed contract.

## Decision

Expose a private mixed-plan tick adapter that maps validated observation, query,
accounting-window, identity capacity and maintenance controls into existing selected
bound runtime cycles. The adapter supplies configured routes, rendering/provider
profiles and exact permission tuples. Credential lookup remains inside resolution,
after the delivery guard. Programs, cycles, owned storage, helper commands, ordinary
send callbacks, clocks and cancellation are caller supplied.

The existing bound cycle has a single SMTP transport timeout. Until it supports
per-destination deadlines, composed ticks reject differing SMTP timeouts before
calling hooks or touching storage. They never substitute an arbitrary timeout.
Common timeout values retain their configured milliseconds. This restriction
applies to the private tick adapter, not the draft validator or bound poll method.
Static credential-resolution errors map to the existing credential diagnostic;
profile mismatch maps to the existing profile diagnostic.

## Evidence and limits

Two tests cover composed runtime behavior. Cancellation before polling creates no
work. An explicitly enrolled SMTP pair gets a controlled child temporary final
reply while a selected ordinary destination completes. After SQLite reopen,
permission denial blocks before credential lookup/helper launch. Recovery with
rotated credentials completes the pending SMTP item. Edited selection neither
retargets its frozen payload nor backfills an already persisted unmatched event.
Frozen bytes survive retry and completion; all three terminal events remain in the
ledger. A second test rejects differing timeouts and missing internal SMTP state
before all hooks and without outbox writes.

The helper reads a private frame and returns synthetic bounded acknowledgments;
it does not establish actual relay receipt. Existing transport acceptance separately
covers TLS and SMTP protocol boundaries. Ordinary send remains injected and does
not establish a public webhook payload. No ambient identity/environment access,
foreground loop, real cluster or installed monitoring is introduced.

## Next

Add per-destination SMTP deadlines to private mixed cycles, then exercise composed
plans with distinct timeout budgets. Public configuration, payload, licensing and
installed activation decisions remain explicit review gates.
