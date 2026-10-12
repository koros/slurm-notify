# ADR 0045: Private shutdown reports across process boundaries

Status: trusted Linux/macOS fixtures; no installed output contract.

Extend existing foreground children to render the private report after cancellation,
finite exhaustion or wait failure. Cancellation uses real shared-signal waiting;
exhaustion uses the existing synthetic-time blocked replay. Held fixtures retain
READY coordination and static output limits. One-shot reporting is supervised with
a two-second deadline and the report's 96-byte output ceiling. Exact fixture decoding
rejects arbitrary diagnostic text and oversized output.

Parent acceptance verifies contention before shutdown, unchanged idle state bytes,
owner release and fresh status. Reported replay preserves the pending event, attempt,
frozen SMTP bytes, provider binding and checkpoint. Exhaustion still leaves blocked
work pending, so its report does not imply completed delivery.

These modes belong only to the trusted custom test executable. No CLI grammar,
exit-code guarantee, OS signal handling, dependency or product default is introduced.

Next: private startup failure classification and bounded metadata-free reports,
including protected-file and owner-acquisition failures before scheduled activation.
Public contracts, licensing and installed activation remain review gates.
