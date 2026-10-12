# Engineering instructions

Use README.md and docs/architecture/implementation-plan.md for current status and
next work. Consult relevant ADRs and proposals when changing those components;
the original handoff and implementation history are historical context.

- Implement focused increments and report behavior, validation and remaining gaps.
  Do not claim installed notifications or supported platforms without acceptance.
- Keep Rust internals private unless a supported API is intentional. Document CLI,
  config, event and persistence semantics and compatibility impact; keep contract
  baselines deliberate and never regenerate them simply to pass checks.
- Explain dependency and operational-default changes. Keep credentials and raw
  private diagnostics out of fixtures, logs and documentation.
- Use subprocess argument arrays, bounded execution/output and resource ownership.
  Preserve privacy, TLS verification and durable recovery guarantees.
- Test meaningful normal, failure and restart behavior. Ordinary tests use synthetic
  scheduler data and controlled provider fixtures without a Slurm cluster.
- Update current documentation and CHANGELOG.md. Historical results belong in the
  implementation history; avoid appending obsolete status claims to README.md.
- Validate site policy before real-cluster monitoring/tests. Publish releases only
  with explicit authorization and verified support/install acceptance.

Install hooks with `sh scripts/install-hooks.sh`. Run `sh scripts/check.sh` for
formatting, strict Clippy, locked Rust/process tests, Python tests and build/smoke
checks. Run `sh scripts/check-coverage.sh` for the 90% minimum Rust line coverage
gate when changing Rust behavior or coverage configuration. Pre-commit checks the
staged snapshot; pre-push runs shared checks. Fix failures; never bypass hooks/CI.

The project uses the MIT license. Product support and public interface baselines
remain work in progress; consult the current review package for unsettled semantics.
