# ADR 0031: Private composed state ownership

Status: private Linux/macOS adapter; no installed path contract.

## Decision

A prepared mixed plan can explicitly acquire owned SQLite state using its stored
path metadata and a caller-supplied SQLite lock timeout. Acquisition delegates to
the existing guarded private namespace: owned 0700 directory, single-link 0600
lock/database files, safe ancestors, no symlink traversal and a nonblocking advisory
owner lock held for the Store lifetime. No unguarded fallback or permission repair
is attempted. Preparation itself still performs no storage writes.

The existing namespace fixes its database basename to state.sqlite3. The adapter
therefore requires that exact basename and a normalized absolute path with no
empty, dot or parent segments, and rejects the root directory as a state parent.
Other draft filenames remain structurally valid but unsupported by this private
acquisition adapter. They are rejected before filesystem access, never silently
redirected. Only the final state directory may be created by existing acquisition.
Arbitrary product paths and platform support remain unapproved.

Ownership contention and unsafe-path failures retain the existing storage error
categories. Acquisition does not poll or dispatch; callers must handle errors
before starting a cycle. A successful Store owns the lock and releases it on drop.
The production adapter does not retry contention automatically.

## Evidence and limits

Two tests verify protected-file composition creates no state until explicit
acquisition, private modes, exclusion of a second owner, successful reopen after
close, unsupported/ambiguous path rejection without directory creation, unsafe
parent rejection, database symlink rejection without changing its target, and lock
release after a failed database check.

The file-backed composed cycle acceptance now uses guarded acquisition for initial,
permission-denied and recovered state. It retains controlled SMTP retry, ordinary
progress, cancellation, rotated credentials, edited routes and exact frozen bytes
through reopen. Test-only reopen helpers bound brief ownership retry to one second,
matching existing namespace tests for descriptors briefly inherited by concurrently
forking test processes before close-on-exec; adapter behavior stays nonblocking.

Existing namespace tests separately cover process death, lock/database/sidecar
symlinks and initialization failure. No real cluster, provider receipt, default path,
installed monitoring or supported network filesystem semantics are claimed.

## Next

Apply independent configured retained-identity and retained-event capacity budgets
to private composed cycle preflight and health, with boundary/recovery acceptance.
Public contracts, licensing and installed activation remain review gates.
