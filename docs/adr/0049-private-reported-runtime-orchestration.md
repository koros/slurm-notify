# ADR 0049: Private reported startup-to-shutdown orchestration

Status: private Linux/macOS adapter; no installed entry point.

A request supplies protected config path, poll/tick budgets and SQLite timeout.
Caller hooks supply identity, resource lookup and ticks. One shared Signal applies
to startup, real monotonic waiting and the tick callback. The adapter returns only
a bounded startup or shutdown summary and writes no output automatically.

Cancellation/poison fails closed. Invalid tick budgets fail before configuration,
resources or state access. Successful preparation delegates to existing owned
session scheduling and shutdown; ownership/resources drop before the summary is
returned. Tick callbacks explicitly wire the supplied signal into composed ticks.
Poll and scheduling budgets remain distinct; exhaustion is not delivery completion.

Two tests cover missing config/cancellation/invalid budgets without callbacks or
state creation, then committed-query cancellation and recovery through the same
adapter. Fresh-signal restart reconciles without duplicate events and delivers the
remaining item exactly once. Owned state is reacquired after both outcomes.

No hook is implicitly selected, synchronous work is not preempted, and no OS signal,
service, public output/config contract, dependency or executable default is introduced.

Next: use the unified adapter in trusted process lifecycle fixtures, including
source replacement, bounded summaries and durable restart recovery.
Public contracts, licensing and installed activation remain review gates.
