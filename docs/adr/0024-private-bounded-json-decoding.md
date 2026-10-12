# ADR 0024: private bounded JSON decoding

Status: private experiment; no approved schema or installed parser.

## Decision

Use test-only pinned serde_json 1.0.152 and serde_core 1.0.229 for a custom
DeserializeSeed/Visitor boundary. This avoids implementing JSON string, escape and
Unicode parsing ourselves. Use the existing locked serde_core version; preserve
unrelated locked versions. The lock adds serde, serde_json, itoa and zmij. No
production dependency, default or executable behavior changes. The upstream parser
interface is documented at https://docs.rs/serde_json/1.0.152/serde_json/struct.Deserializer.html.

Before parsing, reject empty/over-65,536-byte input, invalid UTF-8/BOM, container
depth over eight and noncanonical, negative, fractional, exponent or overflowing
integer tokens. Quoted brackets/numbers do not affect these budgets. Keep the
parser's own recursion protection enabled and reject trailing input.

The visitor retains an object-root tree with at most 128 members/elements per
container. Compare decoded keys so escaped duplicate spellings fail. Reject oversized
collections rather than truncate. No Debug/Clone is implemented for the tree;
retained decoded text uses Zeroizing owners. Errors expose only static categories.
The borrowed source is caller-owned; protected snapshots already use zeroizing
owners. Parser scratch allocations and transient decoded strings are not certified
as zeroized by this wrapper. It does not accept inline credentials as a product policy.

## Evidence and limits

Three tests cover both draft examples and independent owned scalar/array values,
escaped duplicates, integer lexemes/overflow, malformed UTF-8/BOM/surrogates,
trailing input, object-root enforcement and exact byte/depth/collection boundaries.
They use synthetic bytes without scheduler, resource resolution or network access.

This is syntax/structure decoding, not full configuration validation. Unknown
fields, schema versions, resource references, typed field relationships and runtime
composition are not checked here. The Python validator remains review tooling.
The draft examples are fixtures, not approved compatibility baselines.

## Next

Add private typed draft-shape validation over the bounded tree, with unknown-field,
version/type/reference rejection before any resource lookup or scheduler query.
Public parser adoption and contract baselines still require explicit review.
