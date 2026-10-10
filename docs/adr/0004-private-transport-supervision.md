# ADR 0004: private transport supervisor prototype

Status: proposed, test-only; no installed transport helper or public IPC contract.

## Decision and scope

A private Linux/macOS supervisor owns one caller-selected child per request. It
passes bounded input over stdin, reads bounded stdout concurrently, discards stderr,
and clears the child environment. Request data is never added to argv, environment,
or diagnostics. The caller must choose a trusted executable and argument list;
tests use Python only as a controlled blocking/acknowledging fixture.

A framing experiment prefixes each byte field with a four-byte big-endian length,
checks total size before allocating, and supports arbitrary payload bytes. It is
not yet decoded by a webhook child. No helper command, endpoint configuration,
credential acquisition, or user-facing protocol is exposed by the executable.

The result experiment accepts only exact OK, RETRY, REJECT, or RETRY_AFTER with
non-negative decimal seconds and a final newline. Successful process exit and
complete bounded input/output are required before any delivered outcome. Extra
bytes, malformed encoding, nonzero exit, oversized output, timeout, and I/O failures
produce static errors with no request or child output in their diagnostics.

The total monotonic request deadline begins before spawning and covers stdin
writes, stdout reads, and waiting for child exit. Writer/reader threads belong to
that request. Cleanup kills the process group, kills/reaps the direct child, and
waits up to one extra second for the pipe threads to finish. It also removes
same-group descendants after a successful direct-child exit.

This is not a real-time guarantee: OS spawn/reap operations are synchronous and
may add latency. Descendants that deliberately leave the process group can escape
cleanup and retain pipe handles; the resulting cleanup error is visible but such
threads may outlive the call. Use one serialized owner and a trusted child. Other
platforms return UnsupportedPlatform until an equivalent cleanup design is tested.
No detached DNS-thread workaround is introduced.

## Evidence and pending integration

Six supervisor tests cover framing/result validation, pre-spawn checks and missing
helper, stdin/environment isolation, blocked input and output, output bounds,
nonzero/malformed results, and descendants holding pipes. One worker/database test
verifies a timeout schedules a retry, survives reopen, keeps its stable event ID,
and completes only after an exact acknowledgment. No dependencies are added.

Timeout or failed helper execution leaves external acceptance uncertain. The worker
must treat that as retry within its attempt budget, preserving at-least-once replay
semantics. The fixture integration verifies this behavior, but the actual blocking
webhook adapter is not yet moved into a child. Its DNS limitation remains until
that integration is complete. Strict input decoding, secure executable selection,
actual HTTP/DNS child tests, and completion-time scheduling are the next increment.
