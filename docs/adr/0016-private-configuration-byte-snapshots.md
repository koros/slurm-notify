# ADR 0016: Private configuration byte snapshots

Status: private experiment; no approved product path, config parser or CLI contract.

## Decision

Add test-only `config_snapshot` to read a caller-selected local file into opaque
zeroizing bytes. Reuse the locked libc and zeroize development dependencies; add
no dependency, environment lookup, file creation or installed command.

On Linux/macOS, require an absolute UTF-8 path of at most 4096 bytes, without
control characters, dot/parent components or a trailing slash. Walk parent
directories using descriptor-relative opens with `O_NOFOLLOW`, `O_DIRECTORY`,
`O_CLOEXEC` and `O_NONBLOCK`. Ancestors must belong to root/effective user and must
not be group/other writable, except existing root-owned sticky directories. The
immediate parent must be effective-user-owned with mode 0700.

Open the final entry read-only, no-follow, close-on-exec and nonblocking. Require
a regular effective-user-owned file, one hard link and exact mode 0600. Do not
follow a special file or FIFO into a blocking stream. Bound the metadata size to
1..65,536 bytes and independently read at most 65,537 bytes to detect growth.
Interrupted reads retry; all read errors are static categories. Buffers and the
returned snapshot are zeroizing and have no Debug/Clone representation.

Before publishing bytes, recheck file type/permissions/link count and compare
descriptor identity, size, mtime and ctime (including nanoseconds) with the initial
metadata. Require the byte count to match the original length. Observed changes
fail without returning a partial snapshot. The returned bytes remain owned even
when the source is later edited or removed.

## Evidence and limits

Five Rust tests cover exact byte/read budgets, interruption and I/O failure,
private-file reopen, immutable returned bytes after editing/removal, unsafe paths,
modes, hard/symbolic links, directories/FIFOs, oversized/empty files and observed
read-time mutation. Fixtures use temporary owned files and the synthetic mixed
draft; there is no scheduler/provider access.

Three additional deterministic acceptance tests replace the opened file atomically,
substitute a symlink after collecting bytes, rewrite the same number of bytes with
a forced distinct timestamp, and change permissions before publication. An unlinked
descriptor fails the one-link check; same-size mutation fails metadata comparison;
changed permissions fail ownership policy. Reopening an atomic replacement reads
the new regular file, while reopening a symlink or permissive file rejects it.
No sleeps, scheduler access or external network calls are required.

This is a byte reader. It does not parse JSON, load referenced trust files/secrets,
establish supported state/config paths or activate monitoring. The Python draft
validator remains a separate review harness; there is no installed Rust-to-Python
configuration pipeline. The private 0700/0600 policy needs product UX review.

Metadata checks detect observable changes; they do not promise an immutable
filesystem snapshot against a writer that can modify the same inode. Byte budgets
do not establish an absolute filesystem wall-clock deadline or certify network
filesystem behavior. A later installed loader must handle those limits explicitly
and preserve the supervised runtime/resource policy. Unsupported platforms return
a static unsupported category without accessing the path.

## Next

Exercise any remaining reader races/resource-loading boundaries privately, then
implement a reviewed parser and config-to-plan mapping. Public schemas, paths,
commands and activation remain gated by explicit contract review.
