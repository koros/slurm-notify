# Engineering instructions

Read SLURM_NOTIFY_DESIGN_AND_CODEX_HANDOFF.md and the current implementation plan
before making changes. Work in the smallest authorized milestone. Do not claim
working notifications until the relevant acceptance criteria have been verified.

- Keep implementation private; public CLI, config, event, and library contracts
  require explicit review before being introduced or changed.
- Explain new dependencies and default changes. Do not embed credentials or
  unredacted logs in fixtures, documentation, or diagnostics.
- Use argument arrays for subprocesses, with bounded execution and resource use.
- Add meaningful tests for implemented behaviour, including relevant failure,
  restart, and edge cases. Ordinary tests must not require Slurm or network access.
- Run cargo fmt --all -- --check, cargo clippy --workspace --all-targets --locked
  -- -D warnings, and cargo test --workspace --locked after Rust changes.
- Update documentation and the changelog. Never bypass CI or silently regenerate
  approved contract snapshots to make checks pass.
- Validate cluster policy before real-cluster monitoring or tests.
- Do not publish releases without explicit authorization. Licensing, support
  baselines, and unresolved design decisions must remain explicit.

Local hooks live under `.githooks`; install with `sh scripts/install-hooks.sh`.
Run `sh scripts/check.sh` for the full shared local/CI suite. Pre-commit checks the
staged snapshot. Do not bypass failing hooks; fix the staged content and rerun.
