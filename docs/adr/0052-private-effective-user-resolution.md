# 0052: Private effective-user resolution

Status: accepted for Linux/macOS private prototype only.

Resolve the effective UID using `geteuid` and reentrant `getpwuid_r` from the already
pinned libc dependency. Do not trust ambient USER/LOGNAME, enumerate accounts, spawn
shell commands or fall back to real UID/current login. No dependencies, public
contracts, installed commands or defaults change.

Validate the caller ceiling (1..256 bytes) before OS access. Use one fixed 65,536-byte
zeroizing scratch allocation; do not resize/retry on ERANGE. Check successful result
pointer, matching UID and name pointer within scratch before accessing the name by
safe slice offset. Require a NUL within the caller ceiling plus one byte. Return an
owned zeroizing identity validated with the observation mapper's existing ASCII,
separator/whitespace/flag-prefix policy. Preserve bytes exactly.

Static errors distinguish bounds, lookup failure, missing account and invalid
identity internally; the injected boundary receives only a unit error. Never include
account names, UID or account-database diagnostics. Scratch can contain other account
fields and is zeroized on every exit. This does not erase OS/NSS-managed copies.

NSS/account resolution can block or use site-configured network services. The memory
cap does not impose a wall-clock deadline; installed activation must address this
limitation. Tests use synthetic bounded records for invalid/oversized/missing values
and a bounded, reaped child for the host lookup and observation-policy composition.
This is host evidence, not a portable cluster support claim. OS signals and installed
identity binding remain separate milestones.
