# ADR 0018: Private injected credential resolution

Status: private experiment; no approved environment/configuration contract.

## Decision

Add test-only `credential_resolution` to resolve SMTP credentials using a caller's
injected lookup function. Reuse the existing zeroizing AUTH credential type without
adding dependencies, ambient environment access or runtime defaults.

Validate both reference names before calling the source: 1..128 ASCII bytes matching
the draft environment-name grammar. Validate the expected identity using the
existing 1..64 printable ASCII byte constraint. Request username bytes with a
64-byte ceiling and require exact equality to the expected identity before looking
up a password. Request password bytes with a 128-byte ceiling; reject empty,
oversized, invalid UTF-8 or non-printable ASCII values using existing AUTH rules.
Do not trim values or provide a fallback.

The lookup source returns owned zeroizing bytes; construction copies accepted
bytes into the existing owned zeroizing credentials. Temporary values drop on
every return path. Static reference/identity/lookup/credential categories carry
neither source messages nor resolved values. Credentials have no Debug/Clone.

## Evidence and limits

Four tests verify exact bytes and ownership after source rotation/removal, request
ceilings, maximum accepted lengths, invalid reference/identity preflight without
lookup, username failure/mismatch without password lookup, and missing, empty,
oversized, invalid Unicode and control-bearing passwords. Fixtures are synthetic;
there is no environment mutation, scheduler access or external network access.

The callback owns source allocation and deadline policy. Passing a ceiling does
not enforce its behavior; the resolver checks returned bytes before publishing
credentials, but cannot bound allocations or wall-clock time inside a callback.
It cannot erase source copies retained by that callback. No supported environment
adapter, keyring/file source, live refresh or child transfer is activated here.
Rotation means resolving a new owned credential value; existing values stay owned.

## Next

Prepare private provider resolution combining owned trust and injected credentials
with the existing immutable provider binding. Installed configuration mapping and
activation still require explicit contract review.
