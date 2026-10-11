# ADR 0009: patch private prototype dependencies

Status: private development decision; no released product support baseline.

## Evidence and decision

A push reported five Dependabot alerts (two critical and three moderate). The
repository alert endpoint requires authentication and was unavailable to this
session; that summary cannot identify individual alert entries or closure state.
An OSV query of every Cargo.lock registry package instead identified three affected
package versions, with paired GitHub/RustSec identifiers:

| Package | Previous | Patched pin | Advisory |
| --- | --- | --- | --- |
| lettre | 0.11.19 | 0.11.22 | [GHSA-4pj9-g833-qx53](https://github.com/advisories/GHSA-4pj9-g833-qx53) / RUSTSEC-2026-0141 |
| idna via url | idna 0.5.0 / url 2.5.2 | idna 1.1.0 / url 2.5.4 | [GHSA-h97m-ww89-6jmq](https://github.com/advisories/GHSA-h97m-ww89-6jmq) / RUSTSEC-2024-0421 |
| time | 0.3.36 | 0.3.47 | [GHSA-r6v5-fh4h-64xc](https://github.com/advisories/GHSA-r6v5-fh4h-64xc) / RUSTSEC-2026-0009 |

The lettre advisory concerns Boring TLS hostname verification. This repository
only enables its message builder and uses its own rustls submission boundary;
the affected feature is disabled. The time advisory concerns RFC 2822 parsing;
the prototype does not expose that parser. Patch both dependencies rather than
preserving affected pins. URL validation now rejects both advisory examples of
Punycode labels decoding to ASCII/empty labels before DNS or network access.

[lettre 0.11.22](https://github.com/lettre/lettre/blob/v0.11.22/Cargo.toml)
requires Rust 1.85 and edition 2024;
[time 0.3.47](https://github.com/time-rs/time/blob/v0.3.47/Cargo.toml)
requires Rust 1.88.0. Pin Rust 1.88.0 for development, CI and matching coverage LLVM
tools. Earlier Rust 1.74 instructions and compatibility conclusions are historical;
this does not establish a public MSRV, binary platform or deployment contract.
The lockfile moves to Cargo format 4. No runtime dependencies are added to the
inactive placeholder executable.

Lettre's builder now uses nom 8 instead of chumsky. Remove obsolete direct
stacker/psm constraints and their resolved packages; related unused allocator-api2
and serde entries also disappear. Time's patched graph updates deranged, num-conv
and time-core. Keep other unrelated pins, including email-encoding/idna_adapter,
unchanged. One test fixture switches to Error::other for strict newer Clippy.
Normal unit/process tests still need no external network or Slurm installation;
security database inspection is a separate network operation.

## Validation and limits

A fresh OSV batch query on 2026-10-11 returned no affected versions among all 79
locked registry packages. This is a point-in-time database result, not a guarantee
against undisclosed issues or confirmation that GitHub has closed its alert entries.
Existing PR dependency review and weekly Dependabot remain active. No alerts are
suppressed or dismissed. Exact message construction, verified TLS, frozen byte
submission/restart, schema and ownership behavior remain covered by shared checks.
New webhook preflight cases reject both advisory Punycode hostnames. Coverage
startup uses cargo-llvm-cov clean --workspace --profraw-only and removes crate
fallback profiles, ensuring each measurement excludes previous runs. Old normal
and fallback profiles carried the earlier LLVM format during the compiler upgrade.
No coverage exclusions or threshold changes are introduced.

Atomic event/outbox/message preparation remains the next implementation milestone.
Public review, licensing, credential lookup and cluster activation remain pending.
