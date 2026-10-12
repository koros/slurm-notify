# ADR 0020: Private webhook resource resolution

Status: private experiment; no approved configuration or environment contract.

## Decision

Add test-only `webhook_resolution` to resolve URL and optional authorization
references through an injected source. Reuse the private reference grammar and
existing webhook endpoint/header validators; add no dependencies or runtime defaults.
Extract endpoint validation from the existing webhook constructor so both paths
use the same HTTPS/loopback, user-info and fragment rules without network access.

Preflight reference names, whole-second timeouts of 1..30 and payload limits of
1024..16,384 before lookup. Request at most 4096 bytes per source and recheck returned
values. Require nonempty ASCII URL bytes without spaces/controls; validate the URL
before requesting authorization. Configured authorization must resolve successfully
to a valid nonempty header; absence means no lookup or header, never a fallback for
a missing configured value.

Keep original resolved bytes in owned zeroizing buffers without Debug/Clone.
Consume them into the existing five-field private transport frame, also zeroizing,
after checking the caller's printable 1..128-byte event identifier and payload
ceiling. Preserve URL/query and header bytes without trimming. No argv, environment
access, child creation, provider permission or network connection occurs here.

## Evidence and limits

Four tests verify exact owned URL/header bytes after source removal, request
ceilings and existing frame decoding, invalid reference/policy rejection without
lookup, endpoint rejection before authorization, static lookup/value failures,
configured authorization without fallback, and event/payload ceilings. Loopback
URLs are private fixture support inherited from the existing transport validator;
they do not approve public HTTP configuration.

The custom supervised HTTP harness now resolves resources through this module
before preparing its requests. The private supervision wrapper accepts owned
AsRef byte buffers so a zeroizing frame remains owned through child stdin transfer;
it delegates to the existing bounded supervisor with the same outcome decoder.
An orchestration fixture blocks missing URL, invalid URL, missing configured auth
and a control-bearing auth value before child creation. Each block skips maintenance
and preserves the same pending event/attempt after store reopen. A decoy loopback
listener receives no connection. Restored resources subsequently pass actual
loopback HTTP 429/Retry-After and 202 recovery, with the same event and body and no
replayed completed work. Existing timeout, TLS and blocked-resolver scenarios use
the owned frames too. This does not establish installed webhook payload semantics.

The injected callback controls allocation and deadline behavior, as in ADR 0018.
Returned-value bounds cannot prevent oversized allocations inside it. Source copies
and URL-parser internal allocations are outside this module's zeroizing ownership;
do not claim complete process-memory erasure. The existing child/client path may
make additional copies. The frame budget does not establish an installed payload,
event identity, source-selection or public schema contract.

## Next

Prepare remaining private configuration-to-runtime mapping boundaries. Installed
parser/configuration mapping and activation remain gated by explicit contract review.
