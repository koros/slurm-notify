# 0051: Private explicit environment resolution

Status: accepted for private prototype only.

The injected SMTP and webhook resource boundary accepts named lookups and byte
ceilings. Add a test-only OS environment adapter without public commands, implicit
names, environment enumeration, fallback, trimming or environment mutation.

Validate the existing reference grammar and a caller ceiling of 1..4096 bytes
before access. Take ownership of OS encoded bytes in zeroizing storage; reject
empty, oversized and non-UTF-8 values. Preserve accepted bytes exactly. Static
internal errors collapse to the resource boundary's unit error without names or
values. Provider validation still enforces its narrower semantics and limits.

The standard-library lookup clones the selected value before its size is known.
The ceiling bounds accepted/retained bytes, not this initial allocation, OS storage
or synchronous lookup duration. Zeroizing the owned allocation does not erase the
process environment. Windows encoded bytes may contain WTF-8; UTF-8 validation
rejects unpaired surrogates. Platform support remains unverified outside tested
hosts.

Tests cover validation-before-access, missing/empty/oversized values, UTF-8 byte
limits, exact whitespace, static errors and Unix non-Unicode values. A bounded,
reaped child with a cleared environment resolves synthetic SMTP credentials and
webhook resources through the actual adapter without querying or sending. No
parent environment mutation is needed. Installed composition, effective-user
resolution and OS signal handling remain separate milestones.
