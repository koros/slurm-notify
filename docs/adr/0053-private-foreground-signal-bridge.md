# 0053: Private foreground OS signal bridge

Status: accepted for Linux/macOS private prototype only.

Bridge process-wide SIGINT and SIGTERM to the existing shared cancellation signal.
Reuse pinned libc; no public commands, dependencies or default changes. The handler
performs only a lock-free AtomicBool store on these targets. A single worker checks
the sticky request every 10 ms and invokes cancellation outside signal context,
waking condition-variable waits. This cadence is not a maximum latency guarantee.

Only one binding may own the process handlers. Install both handlers with
SA_RESTART, retaining prior actions; rollback the first if the second fails. Thread
creation failure restores both. Drop stops and joins the worker and restores prior
handlers. If restoration fails, retain the ownership reservation to reject rebind;
Drop cannot report that error. Process shutdown should treat such an OS failure as
an unresolved lifecycle condition before installed activation.

The owner must outlive foreground execution and be dropped afterwards. Do not
call mutex-based cancellation directly from a handler. Do not alter signal masks:
blocked inherited signals cannot trigger the bridge. Competing handler installers
are outside this private ownership model; applications with other signal managers
need explicit coordination before adoption. Handler state has static lifetime and
references no dropped signal, descriptor or worker allocation.

A bounded, reaped isolated child verifies SIGINT/SIGTERM wake a real shared wait,
repeated signals remain sticky, concurrent binding fails, prior handlers are restored
and fresh binding starts uncancelled. Parent handlers/environment are untouched.
Synthetic runtime tests remain separate: next compose OS signals with owned
foreground state, bounded reports and durable reopen/recovery process acceptance.
No installed notification agent or supported cluster/platform matrix is established.
