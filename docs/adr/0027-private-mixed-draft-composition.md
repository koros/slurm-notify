# ADR 0027: Private mixed draft composition

Status: private prototype; no public contract or installed activation.

## Decision

Compose validated mixed drafts into owned common observation/control/webhook
resources, SMTP rendering and provider descriptors, protected DER snapshots and
explicit permission entries. Full draft validation precedes all resource access;
all rendering metadata is checked before trust reads. Each SMTP profile is checked
before its trust files are read. Identity and webhook resolution follow SMTP trust
preparation. No SMTP credentials are looked up during preparation.

Reference identifiers use a provisional private SHA-256 encoding: a versioned
prefix, purpose tag and ordered length-prefixed UTF-8 trust paths or username and
password environment-reference names. Actual trust content is independently bound
by its fingerprint. Secret values are excluded so password rotation preserves the
provider binding. This encoding is not an approved enrollment interface or a
privacy guarantee for reference names.

Permissions copy the exact configured cluster/destination/rendering/provider tuple;
composition never grants permission automatically. The mixed poll method uses
validated identity, controls and event selection with existing atomic frozen-message
and profile persistence. The common plan stays private inside the mixed plan, so
callers use bound preparation rather than ordinary routed polling for SMTP.

Delivery must pass through the existing exact-pair guard before calling the private
resolver. Resolution checks the current descriptor against the durable binding
before injected credential lookup and validates returned credentials and owned
trust content. A running plan owns its trust snapshot; re-preparation after restart
loads current files and rejects changed trust or reference bindings. Programs,
windows, storage ownership, clocks and lookup deadlines remain caller supplied.

## Evidence and limits

Four network-free tests exercise validation/trust failure before lookup, ownership,
explicit permission preservation, stable bindings across credential rotation,
changed reference/trust rejection before lookup, atomic selected polling, permission
denial before resolution and exact frozen-message preservation after SQLite reopen.
An unmatched terminal event remains deduplicated without creating delivery work.

This milestone does not connect composed drafts to supervised transport or runtime
cycles. Existing lower-level acceptance covers those stages separately. Installed
identity/environment adapters, filesystem activation, public config/payload review,
permission enrollment and licensing decisions remain outstanding.

## Next

Exercise composed mixed drafts through private runtime-cycle and supervised
transport acceptance, retaining the public-contract review gate.
