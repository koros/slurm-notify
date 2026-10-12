# 0054: Private owned foreground OS signal acceptance

Status: accepted for private Linux/macOS process fixtures only.

Compose the signal bridge with the unified protected startup-to-shutdown runtime.
Trusted children bind before startup, use injected synthetic identity/resources,
commit a blocked SMTP tick and emit READY. The parent verifies owner contention,
then sends SIGINT or SIGTERM to its owned child PID. No installed flags, real Slurm
queries, credentials, provider sending, defaults or dependencies are introduced.

The shared bridge interrupts foreground scheduling after one committed tick.
Unified orchestration closes state ownership; the child drops the bridge, verifies
fresh binding/worker cleanup and emits the existing exact bounded cancellation
summary. Parent supervision retains the two-second deadline, 64-byte output cap,
reaping and assertion-failure cleanup used by the existing lifecycle fixtures.

For both OS signals, compare state file names/content before and after shutdown.
Reacquire and verify frozen message bytes, provider token, attempt number and poll
checkpoint. Synthetic reopen and bounded-report replay remain blocked with no new
events or rewritten pending work. Status can reacquire after child exit. Existing
forced-termination recovery remains covered separately.

This proves signals at the READY boundary after durable blocked work. It does not
prove interruption of synchronous NSS/resource callbacks or every active transport
stage. Signal masks, competing handlers and non-guaranteed bridge wake latency
remain documented in ADR 0053. Next extend OS signal acceptance to active query/send
callbacks and partial-delivery recovery; public/installed activation remains gated.
