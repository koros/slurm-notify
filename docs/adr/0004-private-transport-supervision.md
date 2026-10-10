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

## HTTP child integration follow-up

A custom Cargo test target (`transport_boundary`, harness=false) now doubles as
its own private child entry point. Its parent resolves its own executable path;
only a fixed child/fixture mode appears in argv. Cargo runs the parent acceptance
suite, and the parent supervises this known executable to send HTTP. This is a test
artifact, not an installed helper, hidden product command, or new public contract.
It includes the existing private modules to exercise their actual implementations;
unused items are allowed in that custom harness because it runs selected scenarios
rather than the ordinary unit-test harness.

The child reads exactly six length-prefixed fields: endpoint, optional authorization,
event ID, raw body, injected Unix timestamp, and socket timeout milliseconds. It
checks cumulative length before each allocation, rejects truncation and trailing
bytes, validates textual fields through the adapter, and writes only a sanitized
outcome. Fixture caps are one MiB total input and at most 60 seconds socket timeout;
these are internal test limits, not approved configuration defaults. Response output
is capped by the parent. Invalid input exits unsuccessfully without logging its
contents. EOF after the sixth field is required; the parent deadline also covers
waiting for that EOF.

Two new unit tests verify strict decoding and invalid child fields before sending.
Seven custom acceptance scenarios verify real loopback HTTP acknowledgment, rate
limit, rejection, slow HTTP termination, unknown TLS issuer rejection, blocked
resolver termination, and malformed-frame rejection. Request body and stable ID
headers are observed at the server. The blocked resolver is explicitly injected in
the child fixture and never invokes external DNS. No lingering resolver thread is
left in the parent; the child is killed and reaped by the existing supervisor.

Completion-time scheduling remains unresolved: timestamps are still injected and
the worker still uses its batch time. Secure installed executable selection,
production IPC limits, status diagnostics, supported-platform policy, and runtime
activation remain further work. The normal executable still reports unavailable
monitoring and cannot send notifications.
