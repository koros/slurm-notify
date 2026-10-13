# 0061: Installed finite SMTP configuration and exact permissions

Date: 2026-10-13
Status: accepted local MVP increment

## Decision

Extend installed `config validate` to SMTP-only and mixed configurations. Reuse
protected trust snapshots and the provider fingerprint codec. Extract local reference,
rendering canonicalization, strict address and credential-field policies into shared
pure modules so validation neither compiles transport orchestration nor opens state.
The private engine delegates to the same routines; its existing canonical bytes and
profile tokens are preserved. Only previously locked ring, rustls and lettre builder
versions move to normal dependencies. No SMTP transport feature is enabled.

Validate the entire JSON shape before resource access. For each SMTP profile,
validate rendering addresses and host/profile metadata before protected trust reads;
parse actual trust contents and compute the current rendering/provider fingerprints.
Require an exact cluster/destination/rendering/provider permission match before that
profile's credential lookup. A denied/mismatched pair exits 3 with static stderr;
invalid config/resources exit 2. Earlier providers may already have had resources
checked. The trust files themselves must be read to establish the actual fingerprint.
Username must match the configured authentication identity and shared ASCII bounds;
reject it before reading a password. No lookup, parse or policy error includes values.

Success reports the safe cluster alias, destination count and permission applicability.
No scheduler, account lookup, network request, state creation or historical queue
inspection occurs. This does not prove TLS handshake or relay acceptance. Snapshot
bytes need not remain alive after finite validation; real monitoring preparation must
retain its separately owned resources and revalidate them when that command starts.

## Compatibility and acceptance

Retire the previous unsupported-SMTP diagnostic deliberately, add permission-denial
snapshot/exit 3, and update help. Existing webhook/version/invocation streams remain
stable in the unreleased 0.0.0 package. Structural `local-v1.schema.json` includes
conditional SMTP permission/preparation fields; Rust enforces relationships, lexical
budgets, references, resource protections and permission semantics beyond JSON Schema.

Mixed-config unit acceptance computes revisions with the private engine and checks
installed validation against those exact tokens. Altered relay, recipient or disclosure
is denied without credential access. Missing/mismatched usernames, invalid passwords
and invalid trust fail statically. Binary acceptance covers approved SMTP-only config,
missing credentials, empty/mismatched permissions, unsafe trust modes, malformed DER,
redaction, unchanged/no-created state and no provider connection. Shared tests continue
covering trust limits/symlinks and fingerprint/recovery behavior.

## Remaining work

Setup must calculate and explain exact enrollment tokens; users should not be asked
to calculate hashes. Pending historical profile reconciliation is separate from finite
current-profile validation. Installed status, setup/test/doctor and foreground monitoring
remain pending, along with public event persistence and permitted external acceptance.
