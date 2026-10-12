# 0060: Installed finite webhook configuration validation

Date: 2026-10-13
Status: accepted local MVP increment

## Decision and compatibility

Activate `config validate --config <absolute-path>` for webhook-only configs under
the adopted local MVP contracts. Preserve existing version, no-argument and invalid
invocation behavior. Expand global help deliberately and add resource-free command
help. One config option may precede, follow or separate command words; unknown,
duplicate and extra arguments fail before config access. This unreleased 0.0.0
addition has reviewed baseline files, not automatic snapshot regeneration.

Reuse protected byte snapshots and the strict decoder/shape validator. Extract
operational range/relationship checks and compiled route validation into shared
pure modules so installed validation does not compile or activate the delivery
engine. Existing private runtime mapping and routing delegate to those same checks.
Only already locked serde_json, serde_core, zeroize, url and libc versions move
into production dependencies; no new package/version is introduced.

Resolve all webhook URL/auth references, requiring bounded printable HTTPS URLs
without userinfo/fragment and bounded printable authorization. Output exposes only
schema version, safe cluster alias, provider count and permission applicability.
Invalid configs/resources emit one static diagnostic, never rejected values/paths.
No scheduler, account lookup, provider request, state creation or implicit config
path is used. Environment lookup allocates before accepted-byte checks; do not
claim a hard bound on initial OS allocation or lookup duration.

SMTP remains required for the MVP. Mixed/SMTP configs receive a specific unsupported
operation diagnostic before environment/resource lookup until rendering/profile
fingerprints, trust/credentials and exact permission inspection are connected.
Status, doctor, test, init and monitoring commands remain unavailable.

## Validation

Installed-binary acceptance covers three option positions, resource-free help,
invalid grammar, missing/unsafe URL/auth values, HTTPS-only policy, userinfo and
fragments, mode-0644 rejection, symlinks, duplicate keys, redacted static errors,
SMTP refusal and unchanged files/no state creation. Existing parser/policy/routing
unit fixtures and shared process acceptance continue to validate the extracted
logic. Structural JSON Schema records the supported webhook shape; duplicate
lexemes, cross-field rules, references, byte/depth budgets, protected files and
resolved resources require Rust validation beyond JSON Schema.
