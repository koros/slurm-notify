# Local MVP adoption record

On 2026-10-13 the maintainer selected "Adopt the proposed local MVP contracts"
for the [review package](../docs/proposals/local-contract-review-v1.md).

Adopted: strict JSON with explicit settings; local foreground monitoring; generic
HTTPS webhook and verified required STARTTLS SMTP; minimal frozen event payloads;
configuration/resource limits, initial suppress/emit choice, additive route selection,
restart-based changes and exact SMTP permission pairs. The corresponding documents
in docs/proposals retain detailed semantics and historical review context.

Help/version is already installed. Adoption authorizes implementation of the other
local commands and creation of their compatibility baselines. It does not claim
that draft example permission hashes enroll a real profile, that cluster polling is
permitted, or that a monitoring binary/release has passed acceptance.

Remaining decisions must be resolved from evidence: supported Slurm/platform matrix,
source timestamp/timezone/identifier limits, historical profile enrollment UX,
exact finite command/status output and contention diagnostics, UUID implementation,
and installation/release/security acceptance. Unknown occurrence/submission/start
times stay null; no fabricated source timestamps. No public Rust API, log monitoring,
remote SSH, implicit TLS, background service or live destination retargeting is adopted.

Changes to adopted semantics require compatibility rationale and explicit baseline
updates. Tests must not regenerate baselines merely to pass.
