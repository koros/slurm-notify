# ADR 0001: Start with a private executable workspace

Status: accepted for the foundation increment; revisit as implementation grows.

## Context

The handoff proposes five component crates but explicitly advises starting small.
No implementation exists, and public library APIs and dependencies are undecided.

## Decision

Use a Cargo workspace containing only `crates/notify-cli`, with `publish = false`,
no dependencies, and no library target. Preserve the other scaffold directories as
potential future boundaries. The executable clearly reports its development status
and exits unsuccessfully; it has no supported CLI contract.

Pin the already available Rust 1.74.0 as the foundation development toolchain.
This is not a decision on the product's minimum Rust version. CI initially checks
Linux only; it does not establish a supported binary or libc baseline.

## Consequences

Build and lint checks run now without establishing accidental library APIs.
Scheduler, storage, and provider boundaries can be introduced when their behaviour
and tests justify them. Monitoring, packaging, and security release gates remain
future work. No scheduler commands or external delivery occur in this increment.
