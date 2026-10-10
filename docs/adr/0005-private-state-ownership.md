# ADR 0005: private state creation and ownership prototype

Status: proposed, test-only Linux/macOS boundary; no approved product state path.

## Decision

A new private Store::open_private entry point accepts an explicit absolute state
DIRECTORY. It creates only the final directory, with mode 0700, and uses fixed
internal names state.sqlite3 and .owner.lock. Existing parents must already exist.
This does not implement the draft configuration's arbitrary storage.path filename;
that interface remains unapproved and needs reconciliation before activation.

Walk ancestors using directory descriptors and openat with O_DIRECTORY/O_NOFOLLOW.
Reject symlink components and parent traversal. Require ancestors owned by root or
the effective user without group/other write access, except root-owned sticky
ancestors such as a system temporary directory. Require the final directory to be
owned by the effective user with exactly 0700 permissions. Tests canonicalize the
OS temporary-directory alias before passing it; callers must supply a symlink-free
absolute path rather than relying on a blanket runtime canonicalization bypass.

Create database and lock files with 0600, O_NOFOLLOW, O_NONBLOCK, and O_CLOEXEC.
Validate opened metadata: regular file, effective-user ownership, exactly 0600
permissions, and one hard link. Reject unsafe existing files instead of silently
chmodding them. Validate existing SQLite journal/WAL/shared-memory sidecars with
the same policy. Restrictive umask settings that prevent the requested owner access
fail safely rather than being globally changed. A failed attempt may leave newly
created private files/directories for inspection; it never deletes existing state.

Acquire LOCK_EX|LOCK_NB on the persistent lock file before SQLite initialization.
Keep that descriptor and the directory descriptor in an owner guard for the entire
connection lifetime. Close SQLite before dropping the guard. Open the database
with SQLite's NOFOLLOW flag, then reuse the existing migration/foreign-key setup.
Initialization failure drops the guard. Never unlink the lock file on close:
unlinking would allow two owners to lock different inodes at the same path.
The OS releases the advisory lock on process exit, including forced termination.
Close-on-exec prevents children retaining the owner descriptor after exec. A
concurrent fork can briefly retain a duplicate before exec; a child that never
execs can retain it until exit. Production lifecycle must supervise such children,
and a transient competing-owner result remains visible. Post-close tests use a
one-second bounded reacquisition check rather than assuming immediate release
while unrelated parallel-suite children are between fork and exec.

## Scope and limits

Advisory locking excludes cooperating users of this entry point. It does not prevent
a separate SQLite client or the original low-level fixture Store::open method from
opening the database; production activation must use only the owned boundary.
The original opener remains for existing isolated transaction fixtures and retains
its prior behavior. Changing it to NOFOLLOW broke macOS temporary-path aliases;
the stricter flag is confined to the symlink-free owned entry point.

Trust the OS, root, and processes running as the effective user. Hostile same-user
processes can rename files or replace paths and bypass this protocol; descriptor
checks do not establish protection against them. Network filesystem locking and
SQLite journaling semantics are not supported or validated by this increment.
Other platforms have no owned entry point until equivalent behavior is tested.
No default path, lock-wait policy, public persistence guarantee, or migration changes
are introduced. Ordinary tests cannot claim a supported platform release baseline.

## Dependency and validation

Use libc 0.2.190 as an explicit dev-dependency, reusing the version already present
in the lockfile. It provides platform constants/types and a narrow documented unsafe
boundary for openat, mkdirat, geteuid, and flock. No new version/package was resolved,
and the normal executable still has no dependencies.

Six tests verify private permissions, competing same-process owners and reopen,
symlink rejection for directory/database/lock/sidecars, broad directory/file modes,
hard-link and relative-path rejection, initialization-failure release, independent
process exclusion/release, and forced-owner termination without a stale lock. Child
fixtures run through the existing bounded supervisor and need no Slurm or external
network. Existing database/restart, delivery, and process suites also pass.
