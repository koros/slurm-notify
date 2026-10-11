# ADR 0019: Private bound resource composition

Status: private experiment; no installed configuration contract or activation.

## Decision

Add test-only `provider_resolution` to compose the private trust-file and injected
credential loaders into the existing resolved SMTP provider type. Reuse locked
dependencies and existing bounds; introduce no runtime defaults.

Require the supplied profile to match the stored immutable provider descriptor
before opening trust files or looking up secrets. Load the explicit trust set
under ADR 0017 and require its content fingerprint to equal the bound fingerprint
before credential lookup. Resolve the exact expected identity and password under
ADR 0018. No fallback to current endpoint, ambient roots or unauthenticated SMTP.

Add an owned-credential constructor to the existing resolved-provider type. It
rechecks the descriptor, credential identity and actual DER fingerprint before
copying trust bytes and moving the zeroizing credentials into the result. It does
not expose plaintext credentials or require converting them back into strings.
Both the existing constructor and this path return the same private transport input.

Errors contain static categories without paths, source messages or resolved values.
The operation neither grants pair permissions nor sends a message. Callers must
retain the existing permission gate before invoking resource resolution.

## Evidence and limits

Five tests cover successful composition, password rotation under the same binding,
owned resources after file removal, changed relay/identity/references and invalid
profile rejection before file lookup, changed/missing trust before credential
lookup, credential failure and constructor rechecks of profile, identity and trust.
Fixtures use temporary files and synthetic certificates; no SMTP, scheduler or
external network access is required.

Two additional pending-delivery acceptance tests compose the actual private
filesystem trust and injected credential loaders through the existing permission
guard. Denial and cancellation skip resolution even when the trust file is absent.
A synthetic retry schedules attempt two; missing trust, changed trust, an empty
password and a wrong username then block without sending or consulting the outcome
clock. Reopening between failures preserves the pending attempt, frozen bytes,
provider binding and scheduler checkpoint. A restored trust file and rotated
password recover the same message on attempt two. Reopening after the synthetic
delivered outcome confirms the work is no longer due. No SMTP connection is made.

Trust and credential sources are read sequentially, without promising a transaction
across external resources. ADRs 0016–0018 retain their metadata, filesystem deadline
and injected-source allocation/deadline limitations. This is local preparation,
not end-to-end installed acceptance or evidence of delivery. It adds no supported
CLI, schema, environment adapter or source-selection policy.

## Next

Exercise composed-resource resolution with bounded supervised transport acceptance.
Public configuration mapping and runtime activation still require explicit review.
