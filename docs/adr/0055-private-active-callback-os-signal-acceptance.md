# 0055: Private active-callback OS signal acceptance

Status: accepted for private Linux/macOS process fixtures only.

Extend unified runtime lifecycle fixtures with SIGINT/SIGTERM during accounting
query and ordinary-send callbacks. Bind the existing signal bridge before startup.
Synthetic callbacks emit READY and wait for shared cancellation with a one-second
deadline. The parent verifies contention, atomically replaces protected config with
malformed bytes, then sends the selected OS signal to its owned child PID.

The callback completes its known result after cancellation. Query completion must
commit one event and frozen blocked SMTP work before delivery/cleanup stops. Send
completion must record the first known success, stop before the second send and
retain exactly one pending attempt. Unified shutdown emits the existing bounded
cancellation report after ownership release and bridge cleanup/fresh rebind checks.

Repeat all four OS-signal cases with independent state/config fixtures, alongside
existing injected cancellation cases. A malformed restart reports preparation
failure without changing state files. Restore config and replay: query recovery
preserves frozen bytes, provider binding, attempt and checkpoint with zero new
events; send recovery delivers exactly the remaining record with zero new events.
Status reacquires after release. Parent reaping/deadlines/output caps are unchanged.

This demonstrates cooperative synthetic callbacks returning known results, not
preemption of arbitrary synchronous callbacks, NSS or real transport I/O. Provider
acceptance uncertainty and replay risk after remote success/local commit failure
remain unchanged. No external queries/sends, dependencies, defaults or installed
contracts are introduced. Next compose private OS identity/environment sources
with unified runtime startup and static failure/recovery acceptance.
