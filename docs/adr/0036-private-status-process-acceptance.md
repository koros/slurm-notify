# ADR 0036: Private status process acceptance and cancellation

Status: private Linux/macOS acceptance; no installed command or output contract.

## Decision

Existing-state inspection samples caller cancellation before state access, after
read-only acquisition and after the coherent snapshot. Cancellation returns a
static error and drops any acquired owner guard. The existing inspection adapter
uses the same path with cancellation disabled.

The custom acceptance harness launches trusted child fixtures with argument arrays,
cleared environments and bounded output. One-shot reads use the existing supervised
transport with a two-second deadline. Held owners and inspections acknowledge their
lock acquisition, then await a controlled input or forced termination. Parent checks
have bounded waits and cleanup reaps children even after assertion failures.

## Evidence and limits

A unit test covers cancellation before missing-state access, after acquisition and
after the read, including subsequent reacquisition and unchanged file contents.
Process acceptance verifies missing state remains absent, pending-state freshness,
contention with another process, normal owner exit, cooperative inspection
cancellation and forced death of both owners and inspections. Reinspection succeeds
and all state filenames and bytes remain unchanged after every exit path.

Child stdout accepts only exact static fixture tokens within 64 bytes; stderr is
suppressed. No identity, credential, path or provider diagnostic crosses that
boundary. The protected configuration includes an unavailable trust resource and
children receive no credential environment. These flags and tokens belong only to
the custom test executable, never the installed product.

Cancellation is cooperative at the specified boundaries. It does not interrupt a
synchronous SQLite operation; the caller supplies its SQLite busy timeout. Filesystem
latency is not certified as a deadline. Controlled held fixtures do not spawn
descendants; the existing supervisor separately covers process-group cleanup.
Advisory locks still require cooperating processes and rollback-only state.

## Next

Compose private foreground lifecycle control with the existing configuration,
owned-cycle and status adapters, keeping cancellation and restart acceptance local.
Public CLI/config/event contracts, licensing and installed activation remain gates.
