# ADR 0025: private draft configuration validation

Status: private experiment, not an approved schema or installed configuration.

## Decision

Validate the bounded Rust JSON tree against the existing local webhook/SMTP draft
shape before resource lookup or scheduler work. Reject unknown/missing fields at
all object boundaries, wrong types and unsupported versions/providers/policies.
Keep the owned tree's zeroizing strings and return a mapped private operational
plan. No Debug/Clone, ambient environment access, file opens, queries or network
requests are introduced. There are no new dependencies or runtime defaults.

Reuse the operational mapper for its numeric bounds, cross-field relationships and
lossless seed conversion, and the route selector for event/destination membership.
Require a canonical decimal seed string. Validate webhook resource variable names;
SMTP host/port/TLS/visibility/reference text, trust-file lists and auth identity;
absolute storage paths; and exact cluster/SMTP-destination permission tuples with
lowercase revision tokens. Empty routes select nothing and empty permissions deny
SMTP; webhook-only configurations reject SMTP-only delivery fields.

The decoded tree is retained rather than copied into another set of owned metadata
structures. Only operational controls are mapped into their runtime types here;
provider/observation/route composition remains a separate milestone.

## Evidence and limits

Five tests cover both synthetic examples, source-independent ownership and mapped
controls, optional authorization and empty policies, object field/type/version
mutation, every numeric manifest upper-limit/type failure and positive minima's
lower failures, five cross-field relationships and u64 seed preservation, route
and permission mismatches/duplicates, trust reference duplication, text and
specific collection limits. Static error categories contain no rejected values.

Text/reference checks are not proof of valid DER content, approved SMTP bindings,
TLS host verification, mailbox rendering, effective-user identity, environment
availability or safe state ownership. Those remain the responsibility of the
existing resource/transport/state boundaries during later composition. No secret
values are loaded by this validator. It does not establish cluster policy or
approved CLI/config/event compatibility; draft examples remain review fixtures.
The upstream decoder's scratch-zeroization limitation remains as recorded in ADR
0024. Public configuration adoption still requires explicit review.

## Next

Compose a validated private draft with injected observation/resource/route mapping
and verify lookup/query ordering on validation failure. Keep installed commands and
public baselines gated on explicit contract review.
