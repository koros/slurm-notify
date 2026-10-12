# ADR 0035: Private inspection of existing state

Status: private Linux/macOS acceptance; no installed status command.

## Decision

Metadata-only status plans can explicitly inspect their configured existing state
with caller-supplied cycle, time and SQLite lock timeout. Reuse one exact private
basename mapping with mixed owned acquisition. Inspection never creates the state
directory, owner lock or database and never runs migrations or repairs permissions.

Existing namespace traversal validates owned private directories, single-link 0600
files and no symlinks. Inspection opens existing files read-only and takes the same
nonblocking exclusive advisory owner lock. Another cooperating owner produces an
explicit AlreadyOwned result; live-owner status reading is not supported here.

Before SQLite opens, inspection rejects any journal/WAL/SHM sidecar and requires a
SQLite header with rollback-journal read/write versions. This avoids opening WAL
state that could create shared-memory artifacts or attempt recovery. SQLite opens
READ_ONLY and NOFOLLOW, sets connection-local query_only and in-memory temporary
storage, and accepts only the existing schema version 4. No initialization path is
called. A status Store retains ownership until the read finishes and drops it on
success or failure. Database error details are mapped to static inspection errors.

## Evidence and limits

Three tests verify absent state is not created, an active owner blocks inspection,
missing lock files are not recreated, successful cold/pending status reads preserve
all state-file names and bytes, and mutation through a read-only Store fails.
Unsafe directories and database symlinks leave their contents/targets unchanged.
Journal/WAL/SHM artifacts, WAL-mode headers, old schema and malformed databases fail
without migration, repair or file changes. Subsequent safe inspection verifies lock
release on earlier failures. Existing composed path/ownership tests use the shared
basename mapping and retain their behavior.

The existing protected configuration reader and coherent health snapshot reader
remain the input/report boundaries. No provider lookup, query or transport callbacks
are introduced. Reads may update filesystem access times; acceptance concerns state
contents and namespace, not metadata timestamps. Advisory locking requires cooperating
processes; network filesystem semantics and live-owner inspection remain unsupported.
No real cluster, installed path/output contract or public status command is claimed.

## Next

Add private process-boundary status inspection acceptance, including owner exit,
inspection cancellation and credential-free diagnostics. Public contracts, licensing
and installed activation remain review gates.
