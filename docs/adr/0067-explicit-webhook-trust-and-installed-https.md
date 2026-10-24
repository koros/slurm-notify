# ADR 0067: Explicit webhook trust and installed HTTPS acceptance

Date: 2026-10-24
Status: implemented.

## Decision

Allow optional webhook `trust_der_files`, using the protected SMTP DER loader's
1..32 roots and 65,536-byte aggregate bound. Explicit roots replace bundled public
roots, never disable certificate/hostname verification. Absence preserves current
verification and endpoint revision identity. Bind a custom set's canonical content
fingerprint into its endpoint revision so pending work cannot switch trust silently.
File content is snapshotted once with configuration resources; helper children
receive bounded owned DER bytes, not filesystem paths or ambient environment.

Extend the private webhook stdin frame with one optional sixth length-prefixed
field containing at most 32 length-prefixed DER roots (65,664 encoded bytes).
Existing five-field requests remain valid. Only the installed child accepts this
extension; legacy fixture decoders remain strict five-field codecs. Parent input
budget increases by the bounded extension; timeout/output/cleanup limits remain.
Reject malformed, empty, excessive or trailing extensions before network access.
This private same-version optional extension is not a public API. No dependencies
or database migrations change. Schema additions are deliberate optional version-1
extensions; existing configuration/output baselines remain valid.

## Evidence

An ephemeral loopback TLS server verifies installed parent/helper HTTP requests.
A 503 result records a retry; a fresh process replays identical public UUID, JSON
and idempotency key and receives 204. A third process does not replay completed
work. Config validation loads explicit roots without networking. Unit acceptance
checks owned bytes and revision changes on trust rotation; installed malformed
frames fail without output or provider activity. Existing TLS rejection tests and
private transaction/revision guards remain required.

Controlled TLS acceptance establishes no real-provider, cluster or supported
platform acceptance. Private endpoint administrators must supply appropriate roots.
