# ADR 0030: Private protected configuration composition

Status: private prototype; no installed path or configuration contract.

## Decision

Add a private path-based mixed-plan preparation adapter. It reads exactly one
bounded protected configuration snapshot, then passes those owned bytes through
existing full validation and composition. Protected read errors remain static
snapshot errors; validation and resource failures retain their existing categories.
No identity or environment lookup occurs before a successful protected read and
validation. The snapshot drops after preparation, including on error; composition
retains owned metadata/resources rather than source bytes or a live configuration
file descriptor.

The adapter reuses current protected-read policy on Linux/macOS: absolute normalized
path, no symlink traversal, owned private parent and single-link regular file with
0600 permissions, bounded bytes and descriptor metadata consistency checks. Other
platforms retain the reader's unsupported result. It does not create configuration
files, select a default path, enroll permissions, open storage or enable monitoring.

A later atomic path replacement cannot alter an already prepared plan. Explicit
re-preparation reads the new file and revalidates it. There is no hot-reload policy.
Protected snapshot tests separately cover replacement during reading; new composition
acceptance replaces the path only after the protected read, inside an injected
identity callback, proving source-independent ownership at that boundary.

## Evidence and limits

Two new tests reject missing files, malformed JSON, oversized files, unsafe file
permissions and symlinks before injected lookup. A valid snapshot survives an
invalid replacement during lookup; a subsequent prepare rejects that replacement.
A later valid timeout edit produces a new plan without modifying the old plan or
binding revisions. Deleting the file does not invalidate owned metadata.

The composed runtime retry/restart test now prepares initial, permission-denied
and recovered plans from atomically replaced 0600 configuration files. It retains
controlled child retry, ordinary progress, cancellation, explicit enrollment,
permission denial before credentials, rotated credentials, edited routes and exact
frozen-message recovery after SQLite reopen.

No installed parsing, ambient effective-user/environment access, relay receipt or
real-cluster monitoring is claimed. Caller-managed lookup deadlines, storage
ownership and foreground lifecycle remain outstanding integration responsibilities.

## Next

Connect composed storage metadata to private owned-state acquisition and recovery,
with contention and unsafe-path acceptance. Public contracts, licensing and
installed activation remain review gates.
