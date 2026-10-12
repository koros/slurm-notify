# ADR 0047: Private startup reports across process boundaries

Status: trusted Linux/macOS fixtures; no installed startup output contract.

Trusted children call reported startup with cleared environments and injected
identity/resources, emitting only bounded static report tokens. Success closes its
session before a fixture-only ready token. Supervision uses a two-second deadline,
96-byte output ceiling and exact decoding; arbitrary/oversized diagnostics fail.

Acceptance covers pre-start cancellation and invalid budgets with missing config;
missing/malformed files, symlinked configuration and unsafe config permissions;
symlinked state and unsafe state-directory permissions; and another cooperating
owner. Absent state stays absent after preparation failures. Unsafe paths/modes
are neither followed nor repaired. Existing state filenames and bytes remain
unchanged across owner contention, unsafe-state rejection and later successful
startup. A separate owner child confirms cross-process lock exclusion and release.

This is synthetic local acceptance, not OS signal handling, cluster policy or real
provider readiness. The success token is a child fixture acknowledgment, not a new
public report grammar. No dependencies, product defaults or installed behavior change.

Next: private lifecycle acceptance combining reported startup, shared-signal ticks,
shutdown reports and durable recovery in one protected-config entry path.
Public contracts, licensing and installed activation remain review gates.
