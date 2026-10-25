# ADR 0072: Portable no-overwrite configuration publication

Date: 2026-10-25
Status: implemented with injected unsupported-operation acceptance; GPFS acceptance pending.

## Problem and decision

A maintainer reported init failing on GPFS after approval with no destination file.
The generic diagnostic could not identify whether file sync or atomic publication
failed. Existing no-overwrite rename flags are not supported by every filesystem.
Do not infer the precise GPFS errno from that report or weaken sync requirements.

Keep same-directory no-replace rename as the first choice. Only for ENOSYS, EINVAL
or EOPNOTSUPP, fall back to `linkat` from the operation's owned, fully written and
synced temporary file into the destination, followed by unlink of the temporary
name. Both operations use the existing protected directory descriptor. Link creation
atomically fails for any existing target, including symlinks. Never fall back to
ordinary overwriting rename, direct destination writes or a check-then-replace race.
Permission, existence, quota and other errors do not trigger this fallback.

Require successful temporary-link removal before success and retain directory sync.
Readers still require a single-link owned mode-0600 regular file. A crash between
link and unlink can leave two names for the same complete file; protected readers
reject it until deliberate inspection/recovery removes the known extra link while
setup is stopped. Never automatically remove the destination or relax link checks.
File sync and directory sync failures remain fatal and are not ignored for GPFS.

## Diagnostics and compatibility

Exit 1 now identifies file sync, publication, published temporary-link cleanup or
published directory-sync failure, without paths, raw errno or private diagnostics.
Other write errors retain the generic inspect-before-retry message. Publication
errors can have uncertain outcomes, so users must inspect before retrying. Successful
creation/help/config/persistence contracts, dependencies and defaults are unchanged.
The fallback requires hard-link support; an unsupported link also fails safely.

## Evidence and limits

Synthetic fault injection exercises every allowed fallback errno with actual local
link/unlink/fsync, single-link private snapshots, existing files/symlinks, concurrent
one-winner publication and cleanup. Other publication errors do not fall back. File
sync failure prevents publication; directory sync failure preserves a complete
inspectable destination and never authorizes replacement. A model of interrupted
two-link state remains blocked by protected readers until deliberate recovery.

No actual GPFS mount, Slurm cluster or real provider is used in these tests. The
maintainer must retry on the HPC to establish GPFS behavior; diagnostics distinguish
remaining filesystem incompatibility without weakening durability guarantees.
