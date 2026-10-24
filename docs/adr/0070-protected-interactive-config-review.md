# ADR 0070: Protected interactive configuration review

Date: 2026-10-25
Status: implemented with installed terminal acceptance.

## Decision

`init --config <absolute-path>` requires terminal stdin. Prompt for an absolute
protected JSON draft file, read at most 65,536 bytes with the existing descriptor-
owned no-symlink reader, and retain one prepared resource snapshot. Draft-file input
avoids canonical terminal line limits on full JSON. No operational defaults are
filled. Missing values/resources fail; drafts use environment references, not inline
secrets. The destination's existing immediate parent must be owned and mode 0700;
ancestors obey the same trust policy as configuration reads. Create no directories.

Show aliases/types, safe webhook origins (no path/query/header), SMTP relay/port/TLS
verification identity, transmitted field names/disclosure, initial-observation
policy and whether routes are empty. Calculate canonical current SMTP rendering/
provider pairs using the same prepared resources as monitoring. Show every pair
and explicitly state that historical pairs are not retained. Only literal `yes`
confirms creation/enrollment. Any other complete answer cancels with exit 0 and no
write. Do not read SMTP credentials, send, poll, create state or start monitoring.
HTTP-only webhook resources fail before configuration publication; foreground
startup also now rejects them before state creation with resource exit 2.

## Atomic publication

Share protected parent traversal with the existing configuration reader. Create a
random, exclusive, no-symlink temporary file relative to the owned parent descriptor,
set mode 0600, write/sync the complete reviewed JSON, then atomically rename without
replacement (`renameat2(RENAME_NOREPLACE)` on Linux, `renameatx_np(RENAME_EXCL)` on
macOS). Sync the directory. Never fall back to an overwriting rename. Concurrent
creation, a file, directory or symlink at the destination prevents publication.
Cleanup removes only a temporary file this operation actually created. A crash can
leave an unpublished private temporary file; a sync error after publication can
leave a complete destination, so write failure tells users to inspect before retry.

No dependencies or configuration defaults change. Existing explicit settings are
preserved; new SMTP permissions replace only the new file's permission list after
confirmation. This command never edits an existing configuration or historical work.
Help/output baselines are deliberate. Resource/input/destination errors exit 2,
write/publication failures exit 1, successful creation or explicit cancellation 0.
No-terminal operation fails before resource access; noninteractive setup is deferred.

## Evidence

Real terminal acceptance covers creation, cancellation, safe-origin/field/policy
review, empty routes, invalid draft, existing/symlink/unsafe/missing-parent rejection,
no provider connection/state, and SMTP enrollment without credentials. Finite
validation independently accepts the resulting SMTP pairs. Unit tests cover bounded
terminal lines, private complete publication, no overwrites/temp remnants and a
concurrent one-winner publication race. Existing reader permission/race tests remain.
