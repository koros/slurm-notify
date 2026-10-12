# ADR 0044: Private foreground shutdown classification

Status: private report adapter; no installed output or process exit-code contract.

## Decision

Classify scheduler results into cancellation, exhaustion or static failure reasons.
Successful counts must match the scheduler's finite budget: cancellation accepts
0 through 1024 and exhaustion 1 through 1024. Invalid counts become a static failure.
Errors report an unknown tick count because existing error results do not retain
completed-call counts; reporting never invents one.

Reports own only an outcome enum and optional bounded integer. Rendering produces
one ASCII line within 96 bytes, using fixed tokens and at most four count digits.
There are no free-form diagnostics, provider responses, identities, paths, resource
references or credentials. The session's reported-run adapter classifies its existing
run result after that path has released ownership. No stdout is written automatically.

## Evidence and limits

Three tests cover valid/invalid count boundaries, every scheduler error classification,
bounded single-line ASCII output, and cancellation/exhaustion/failure from owned
sessions. Session status is closed and state can be reacquired after each outcome;
reports contain none of the fixture's identity, cluster or storage metadata.

Cancellation means the run stopped; exhaustion means its tick budget ended. Neither
claims all deliveries completed. Failure reasons classify scheduler boundaries and
preserve existing stage reports for callers rather than forwarding their details.
No supported output grammar, exit codes, installed CLI behavior, dependencies or
product defaults change.

## Next

Exercise bounded shutdown reporting across trusted foreground child boundaries for
cancellation, exhaustion and failure while retaining state recovery acceptance.
Public contracts, licensing and installed activation remain review gates.
