# 0058: Private continuous scheduler and first signal reason

Date: 2026-10-13
Status: accepted internal implementation; no public monitoring activation

## Decision

Share one scheduling engine between existing finite runs and a new continuous
entry point. Continuous runs retain monotonic clock history across the former
1,024-tick boundary rather than restarting finite schedules. Every tick is followed
by a completion-relative delay, including blocked/slow ticks. The existing interval,
clock-regression, deadline-overflow and 1,024-partial-wake guards apply unchanged.
Cancellation and failures terminate; the diagnostic tick count saturates at usize's
maximum instead of overflowing. Existing finite budget and exit semantics remain.

Store the first delivered SIGINT/SIGTERM handler's signal number with a native
atomic compare/exchange. Concurrent handlers have no chronological guarantee;
the first successful compare/exchange wins. Later signals request the same sticky
cancellation and cannot replace the reason. Each new owned binding resets the
reason and still restores previous handlers on drop. Signal handlers allocate
nothing and perform no locking or I/O. Callers may inspect the reason before drop;
public exit mapping remains a separate installed-runtime binding decision.

## Validation and remaining work

Injected-time tests run 1,050 continuous ticks with slow ticks and exact fixed
delays; a regression after tick 1,024 remains detectable. A nanosecond-progress
wait fails at the wake budget. Existing finite failure/cancellation checks exercise
the shared engine. Isolated process tests cover repeated and opposing signals,
first-reason retention, restoration, exclusive binding and reset on rebind.

Continuous scheduling is not yet connected to owned sessions. Their finite poll
budget requires explicit renewal preserving health, ownership and sticky cancellation.
Webhook-only sessions, frozen public event persistence, installed config/status,
setup/test/doctor and real-cluster/provider acceptance remain separate MVP work.
