# ADR 0046: Private bounded startup failure reports

Status: private Linux/macOS reporting; no installed output/exit-code contract.

## Decision and evidence

A reported startup adapter delegates to existing session startup and maps failures
into owned static outcomes: cancelled, invalid limit, preparation or ownership.
Non-startup error variants map to an invalid-phase classification rather than
misrepresenting their origin. Success returns the owned session unchanged.

Rendering contains only a fixed startup token and newline within the existing
96-byte ceiling. No paths, cluster/user identities, credential references or nested
errors enter reports. Preparation intentionally combines protected-file, validation
and resource failures; ownership combines acquisition failures without disclosing
namespace details. No stdout is emitted automatically.

Two tests cover all classifications and bounded ASCII rendering; missing files,
invalid limits and cancellation before callbacks; malformed protected input; a
second owner rejected while the original remains usable; and successful acquisition
and closure after ownership release. Existing ordering and state semantics remain.

Startup cancellation after writable acquisition may still leave initialized state;
reporting neither rolls it back nor claims no side effects. This does not introduce
a public grammar, configuration change, new dependency or executable behavior.

Next: trusted process-boundary startup reports for protected-file, owner contention,
unsafe state and cancellation failures, verifying unchanged state and redaction.
Public contracts, licensing and installed activation remain review gates.
