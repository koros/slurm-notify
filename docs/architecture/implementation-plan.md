# Implementation plan

## Assessment

The initial directory contained only the design handoff. The scaffold added
reserved directories and a README. There is no Git repository, configured remote,
approved public contract, license, scheduler fixture set, or monitoring code.
Rust 1.74.0 is available locally.

## Current increment: Phase 0 foundation

Scope authorized by the request to plan and continue:

1. Create one private executable Cargo workspace with no dependencies.
2. Add contributor/agent guidance, changelog, and a license decision placeholder.
3. Record the workspace decision and contract governance approach.
4. Add basic Linux CI and verify formatting, linting, tests, build, and the
   executable's explicit unavailable status locally.

Acceptance: a locked build succeeds; formatter and strict Clippy pass; Cargo's
empty test harness runs; the executable reports unavailable monitoring and exits
with status 1. No public library API, provider, configuration, or CLI proposal is
implemented. Hosted CI execution must be verified after repository setup.

## Remaining foundation work

- Decide licensing and contribution governance; then write LICENSE and conduct policy.
- Configure Git/GitHub, real maintainer ownership, private vulnerability reporting,
  branch protection, and required reviews/checks.
- Agree support baselines, first MVP provider scope, and contract semantics.
- Define scheduler fixtures and meaningful behaviour tests with the first adapter.
- Add threat model, dependency/security checks, and compatibility checks as their
  subjects exist. The basic workflow is not the complete release gate suite.

## Next implementation increment: lifecycle discovery and reconciliation

Plan this as a small, mock-driven part of Phase 1, before external delivery:

1. Agree job identity, normalized scheduler records, initial-scan behaviour,
   historical catch-up window, and accounting-lag handling.
2. Capture synthetic `squeue`/`sacct` fixtures for success, malformed output,
   missing commands, arrays, requeue, and delayed accounting.
3. Implement a scheduler boundary and parsers with bounded subprocess execution,
   explicit fields, safe arguments, and current-user filtering.
4. Implement lifecycle reconciliation and stable event identity behind private
   interfaces; verify short jobs and duplicate observations.
5. Document limitations and approve event/config contracts before webhook exposure.

Acceptance: mock observations produce expected lifecycle events, repeated records
produce no new event, and failures are diagnosable without a real cluster.
Persistence/restart recovery, delivery, and CLI onboarding follow as separate
increments; this slice is not a complete MVP.

## Decisions that block later implementation

Default triggers, initial scan semantics, catch-up window, event identity/schema,
retention/redaction, dependency choices, and provider scope need explicit decisions.
Cluster-specific polling limits, accounting availability, Slurm command features,
and permission to run background processes need validation at the intended site.
No Lovelace compatibility or policy compliance has been established.

## Subsequent increment: private lifecycle experiment

Implemented a test-only accounting parser and in-memory reconciler in `notify-cli`,
with eight tests. Synthetic fields are Cluster, JobID, User, Submit, Start, State,
and ExitCode. This is not a supported wire schema or a validated command invocation.
No dependencies or public interfaces were introduced.

Provisional rules: filter by caller-specified user; ignore step records; reject
unsupported identities and malformed snapshots; keep unknown states unresolved;
emit running/terminal transitions once per cluster/job/submission/start context.
The caller explicitly chooses whether newly discovered states generate events.
The first terminal observation wins; accounting corrections need a future policy.
Missing records never imply completion. Deduplication is in-memory only.

Limitations: no squeue parser, real subprocess boundary/timeouts, persistence,
restart recovery, retention, timestamp validation, heterogeneous jobs, compressed
array IDs, or broader state mapping. Start-context changes can distinguish attempts
but do not establish definitive requeue semantics. The module is compiled only for
tests until these decisions and the scheduler boundary are ready.

Reference: https://slurm.schedmd.com/sacct.html and
https://slurm.schedmd.com/job_state_codes.html. These are current upstream references,
not a claim of compatibility with any particular cluster version.

Next: decide the scheduler record identity and initial/catch-up semantics, then
add the bounded scheduler transport and active-job parser with failure fixtures.

## Subsequent increment: active parser and bounded direct-process runner

Added a test-only active-job parser for JobID, User, Submit, Start, and State,
plus an `squeue` command builder using literal argument arrays, explicit fields,
array expansion, and local scope. Active records preserve an unknown exit code.
Reference: https://slurm.schedmd.com/squeue.html.

The runner uses caller-supplied timeouts and per-stream byte limits, drains stdout
and stderr concurrently, kills/reaps the direct child on failure, distinguishes
missing executables and nonzero exits, rejects invalid UTF-8, and suppresses stderr
contents in errors. It sets C locale and UTC; other environment settings remain
inherited. Tests invoke isolated helper processes from the test executable, without
Slurm, shell scripts, dependencies, or network access.

Validation: 14 tests passed; one helper test is intentionally ignored in the main
suite and invoked by subprocess tests. Formatting, strict Clippy, and locked build
pass locally. Hosted Linux CI remains unverified.

Limits: only direct children are terminated; descendant process trees and inherited
pipe handles are not yet managed, so this boundary must not be used for wrappers
or SSH. Reader threads can outlive the call if descendants retain pipes. Calls
must be serialized by the future polling owner. The parser and runner remain
compiled only for tests. No scheduler query has been made against a real cluster.

Pending `squeue` Start may be an expected time. Active records are therefore not
yet merged with accounting records or fed into a polling loop. A consistent
submission/attempt identity, query environment policy, process-tree cleanup,
bounded retention, historical window, and initial/catch-up semantics remain to
be settled before operational monitoring.

## Subsequent increment: provisional identity and snapshot reconciliation

Pending observations now discard the expected Start field for identity. The first
actual attempt inherits the pending allocation's discovery history, so changing
estimates do not create repeated allocations or suppress a later start event.
Actual attempts retain cluster, job/array task, submission, and actual start.
Active/accounting observations of the same attempt deduplicate together.

The snapshot boundary parses both sources before mutating state and processes
accounting first; a malformed source cannot partially consume events, and stale
active output cannot reverse a terminal accounting observation. Tests cover
pending estimates, delayed accounting, reused job IDs, ambiguous identity, and
atomic parse failures. This updates the earlier provisional identity notes.

Limitations: records with unknown actual start/submission are rejected. This means
cancel-before-start and some incomplete accounting records remain unsupported,
with explicit parse failure rather than guessed identity. Timestamps are not yet
validated, attempt start changes remain provisional, terminal corrections still
need policy, and state is neither durable nor bounded by retention. The snapshot
prototype does not invoke the runner or establish polling/catch-up defaults.
Process-tree cleanup and safe query-environment handling remain outstanding.

Validation: 19 tests pass; one subprocess helper is intentionally ignored in the
main suite. No new dependencies, exported library API, or CLI contract were added.

## Subsequent increment: pre-start cancellation

The private prototype now accepts CANCELLED records whose Start is explicitly
`N/A`, using cluster/job/submission identity with no start context. An observed
pending allocation can transition to this cancellation exactly once. A cancellation
first discovered in history still obeys the caller's explicit catch-up choice.
This is a provisional synthetic-format rule, not validated cluster behaviour.

If the same allocation has a known actual-start attempt in existing state or the
incoming snapshot, a start-less cancellation is ambiguous. The snapshot rejects
it atomically rather than assigning it to a running attempt or producing a second
identity. Unknown submission/start markers remain errors; no general missing-time
fallback has been introduced. Jobs cancelled after starting use their actual-start
context as before.

Validation: 22 tests pass, including pending cancellation, historical discovery,
deduplication, and ambiguity in existing/incoming state. One subprocess helper
remains intentionally ignored in the main suite. Formatter, strict Clippy, and
locked build pass. The executable still performs no monitoring or delivery.

Next engineering work: process-tree cleanup and query environment controls, then
connect the mocked query/parse/reconcile path. Durable storage and restart recovery
remain separate milestones requiring a reviewed dependency and persistence design.

## Subsequent increment: process groups and query environment

On Linux/macOS, the test-only runner creates a fresh process group and signals
that group during cleanup. A small private C ABI call to kill(2) introduces no
package dependency; it checks the group ID and documents the unsafe integer-only
boundary and platform-specific SIGKILL value. The runner waits for the direct
child and allows up to one additional second for reader completion, returning a
cleanup error if pipes remain held. A descendant helper test checks timeout and
reader completion, not just termination of the immediate child.

The runner removes inherited and explicitly configured SQUEUE_*/SACCT_* overrides,
SLURM_TIME_FORMAT, and SLURM_BITSTR_LEN, then sets C locale and UTC. Site connection
settings such as SLURM_CONF remain inherited; tests verify this distinction.

Validation: 24 tests pass; one subprocess helper is intentionally ignored in the
main suite. Formatting, strict Clippy, and locked build pass on local macOS.
Linux hosted CI has not been run. Reference for process-group creation:
https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.process_group.

Limits: descendants that deliberately leave the process group are not controlled.
Other platforms retain direct-child cleanup and are not supported operationally.
The runner still has no production entry point, and cleanup signaling failures
are followed by direct-child cleanup and bounded reader verification. Windows
process-tree handling, persistence, and live-cluster validation remain future work.

## Subsequent increment: mock polling pipeline

Connected the command builders, injectable query boundary, parsers, and reconciler.
Each poll queries active jobs then accounting; reconciliation occurs only after
both queries and both parsers succeed. Query failures retain their source and do
not consume events from a partially successful poll. The direct-process runner
can be passed to the same boundary, but there is no executable monitoring command.

Accounting requests explicitly specify user, allocation-only records, expanded
arrays, duplicates, local scope, seven fields, and caller-supplied start/end times.
There is no default historical window or polling interval. The second query gets
the remaining query budget; output limits are enforced by the injected runner.
Cleanup can add its documented extra second per failed query. Input/window
validation and capability detection remain necessary before live operation.

Tests exercise running/completed observations across repeated polls, delayed
accounting, a failed second query followed by successful recovery, malformed
accounting, and injection of the real runner against a deliberately missing path.
No real scheduler commands were invoked. No external notifications were sent.

Validation: 28 tests pass, plus one helper intentionally ignored in the main suite;
formatting, strict Clippy, and locked build pass locally. Hosted CI and cluster
compatibility remain unverified. The pipeline remains compiled only for tests.

Next: timestamp/window validation and broader terminal-state coverage, followed by
bounded state retention and a reviewed durable-storage design. Public configuration,
CLI, event schemas, dependencies, and operational defaults still require decisions.

## Subsequent increment: query validation and retained-state limits

Poll requests now validate fixed-width calendar timestamps (`YYYY-MM-DDTHH:MM:SS`,
interpreted in UTC by the runner), including Gregorian leap-year rules. Invalid,
equal/reversed windows, zero timeout/output limit, and empty user/cluster fail
before queries execute. Relative dates and timezone suffixes are deliberately not
accepted by this private boundary. This validates query windows only; synthetic
record identity timestamps remain opaque and need validation before production.

NODE_FAIL, BOOT_FAIL, and DEADLINE now retain distinct terminal classifications.
PREEMPTED remains unresolved because its subsequent state depends on cluster policy.
Reference: https://slurm.schedmd.com/job_state_codes.html.

An explicit reconciler capacity can bound retained job identities and event ledger
entries independently. Snapshot updates run on a candidate and commit only if both
limits hold. Exhaustion raises an error without deleting deduplication history or
partially consuming events. This is a capacity gate, not time-based retention;
no expiration policy or operational default is approved. Existing isolated tests
may still use the unbounded default constructor. Candidate snapshots temporarily
allocate before the cap check; runner output bounds constrain input, not a hard
peak-memory limit. Durable retention remains future work.

Validation: 32 tests pass, plus one intentionally ignored subprocess helper.
Formatter, strict Clippy, and locked build pass locally. No public interfaces or
dependencies were added; the executable still cannot monitor jobs.

Next: design SQLite transactions, restart checkpoints, event/outbox atomicity,
migrations, and retention guarantees, then assess the storage dependency before
implementing the durable layer. Operational CLI/config/event decisions remain open.

## Subsequent increment: durable storage design and SQL experiment

Added ADR 0002 and a private SQLite migration experiment. Eight standard-library
Python tests verify atomic event/outbox/checkpoint commits, rollback, connection
reopen, uniqueness, foreign keys, delivery-state atomicity, and version/migration
failure handling. CI now invokes these tests separately from Cargo. No Rust or
Python package dependency was added, and notify-storage remains a reserved directory,
not a Cargo package. SQL tests do not implement application restart recovery.

The proposed transaction design specifies commit before sending or updating live
reconciliation state, stable per-destination delivery records, and explicit
at-least-once limitations. Retention, secure file handling, journal mode, timestamp
units, and concrete Rust dependency/version selection remain pending.

Validation: eight SQLite tests pass locally using SQLite 3.45.3. Rust's 32 tests,
formatter, strict Clippy, and locked build still pass. Hosted CI remains unverified.

Next: assess and adopt a compatible Rust SQLite dependency, implement private
storage operations with transaction rollback/reopen tests, then connect persisted
state to the mock polling pipeline. Public contracts and operational defaults
remain undecided.

## Subsequent increment: private Rust SQLite operations

Added a test-only storage module with rusqlite 0.31.0 (bundled SQLite) as a
strictly pinned dev-dependency. Version 0.32.1 failed on the development toolchain;
the selected version and locked dependency graph compile on Rust 1.74 locally.
See ADR 0002 for dependency rationale, metadata license assessment, native compiler
requirements, and remaining release security/compatibility assessment.

Implemented foreign-key enablement, bounded lock waiting, migration/version checks,
and atomic job/event/outbox/checkpoint recording. Replays preserve original routes;
conflicting logical event IDs fail and roll back. Suppressed events can be persisted
with no destinations. All interfaces remain private and test-only.

Validation: 38 Rust tests and eight Python SQLite tests pass; formatting, strict
Clippy, and locked build pass. Normal executable builds remain dependency-free.
Hosted CI has not been run. Reopen tests exercise persisted event/outbox records,
not restoration of the live reconciler or real process-crash recovery.

Next: connect candidate reconciliation state and stable event IDs to transaction
commit, restore deduplication after reopen, and test failed commits/restart across
the mock pipeline. Secure production paths and delivery operations remain pending.

## Contributor hooks and shared validation

Added dependency-free, version-controlled pre-commit/pre-push hooks and an
installer that sets repository-local core.hooksPath while preserving existing
custom hook configuration. Pre-commit validates index blobs and checks a temporary
export of the staged content; pre-push runs the shared suite in the working tree.
CI independently invokes tracked-file checks and the same validation script.

Guardrails cover common credential patterns, credential filenames, merge markers,
trailing whitespace, and unsupported symlinks/submodules in staged snapshots.
Six isolated Git-repository tests cover staged-vs-working-tree content, redacted
failure output, markdown hard breaks, disallowed files, and symlink rejection.
They clear inherited Git environment variables to avoid using the parent index.

Local validation: 38 Rust tests, eight SQLite tests, and six hook tests pass,
alongside formatting, strict Clippy, build, and executable smoke checks. Local
hooks are installed in this checkout. Each clone requires its own install step;
GitHub branch protection/required reviews remain separate repository settings.

## Current increment: durable mock-pipeline recovery

Connected the mock query/reconcile pipeline to candidate-state SQLite commits.
Live memory is updated only after snapshots, stable ledger IDs, routed outbox
records, and checkpoint commit successfully. Restoration reads a consistent,
capacity-limited snapshot and verifies event identity/reference consistency.
Suppressed initial events stay suppressed after reopen. Pending identity promotion
removes obsolete database rows without discarding historical attempts.

Checkpoint regression now fails atomically. Query and storage failure tests verify
that retry still emits the expected event and the failed poll advances neither
memory nor durable checkpoint. Deterministic length-prefixed IDs remain private;
no public CLI, event schema, configuration, or dependency change was introduced.

Validation: 46 Rust tests, eight SQLite tests, and six hook tests pass. The shared
formatter, strict Clippy, build, and executable smoke checks pass. Local hooks and
CI continue to run the shared suite. Close/reopen recovery is verified; real process
crashes, hosted CI, and live-cluster execution are not claimed by this increment.

Next: committed outbox reads, atomic delivery outcomes/retry state, and mock provider
integration. Production runtime activation also requires secure database creation,
public interface decisions, a current dependency/security assessment, and explicit
operational defaults. The executable still reports unavailable monitoring.

## Current increment: committed outbox and mock provider delivery

Added bounded due-record reads and atomic delivery attempt/outbox updates. A private
worker executes injected providers outside transactions, isolates destination
rejections, schedules capped retries with caller-specified limits, and retires
exhausted delivery records. Stale results and transaction failures do not partially
update the outbox. No schema migration, dependency, or public contract changed.

Eight tests verify timed retry/reopen, stable event identity, destination isolation,
attempt/batch bounds, lowered budgets, atomic failed-result recovery, stale attempts,
and invalid/overflowing policy. One test explicitly permits replay after provider
acceptance followed by failed local commit; at-least-once semantics remain intentional.

Validation: 54 Rust tests, eight SQLite tests, and six hook tests pass, with formatting,
strict Clippy, build, and executable smoke checks. No real delivery has occurred.

Next: define the first private webhook adapter boundary, request/response classification,
timeouts, bounded responses, redacted diagnostics, and Retry-After handling with
local mock HTTP tests. Public event payload/config semantics require explicit
review before exposure. Runtime activation, current dependency assessment, secure
state creation, and real-cluster validation remain pending.

## Current increment: durable provider-requested retry delays

Extended the private injected-provider worker to accept relative delay hints,
persist their due times, preserve the exponential backoff floor, and enforce the
existing attempt budget. Negative hints and arithmetic overflow fail visibly
without partially recording an attempt. Three new tests bring validation to
57 passing Rust tests, eight SQLite tests, and six hook tests. Formatting, strict
Clippy, build, and executable smoke checks pass.

A test-only HTTP client experiment was deferred: ureq 2.12.1 resolved zeroize
1.9.1, whose edition-2024 manifest cannot be parsed by pinned Cargo 1.74. The
experiment was removed from the manifest and lockfile. No dependency or toolchain
change is included. Next: assess a compatible HTTP/TLS dependency set, then add
local mock-server tests for timeouts, status classification, sanitized diagnostics,
and Retry-After delay/date parsing. Public payload/config approval and runtime
activation remain separate pending work. The executable still reports unavailable
monitoring.

## Current increment: private webhook transport

Implemented the private caller-byte POST adapter and local HTTP mock tests.
Seven added tests cover input validation, request headers/body, response outcomes,
retry instructions, redirect refusal, refused connections, socket timeout, and
integrated durable retry/reopen/acknowledgment with a stable event ID. Validation:
64 passing Rust tests, eight SQLite tests, and six hook tests, plus formatting,
strict Clippy, locked build, and executable smoke checks.

ADR 0003 records the compatible test-only dependency pins, client response-header
bounds, TLS assumptions, and synchronous DNS timeout limitation. The normal
executable still has no dependencies and cannot monitor or notify. No public
contract or operational defaults were introduced.

Next: resolve the strict transport deadline and actual completion-time boundary,
then test HTTPS certificate validation and establish a proposed normalized payload
for review. Runtime activation still requires credential handling, secure state
creation, routing/config/CLI contracts, and operational policy decisions.

## Current increment: HTTPS verification and deadline evidence

Five new tests verify trusted HTTPS delivery, unknown issuer/name/expiry rejection,
and the synchronous resolver timeout limitation. The full suite now has 69 passing
Rust tests, eight SQLite tests, and six hook tests. Test-only certificate-generation
and TLS dependencies are documented in ADR 0003; no keys or credentials are stored.
Formatting, strict Clippy, locked build, and placeholder smoke checks pass.

The strict deadline remains unimplemented. A detached timeout thread would retain
blocked DNS work and consume resources across retries. The next private increment
should supervise a transport child process, with bounded framed stdin for endpoint,
secret, ID, and body; bounded sanitized result output; a total monotonic deadline;
and kill/reap cleanup using the existing runner approach. Keep secrets out of argv,
environment, stderr, and diagnostic output. Child exit or timeout must yield an
uncertain retry outcome with the same event ID, never a success acknowledgment.
Verify blocked resolution, blocked input/output, malformed responses, cleanup, and
restart before runtime activation. Choose and document supported-platform process
cleanup behavior rather than assuming Unix guarantees elsewhere.

The worker also needs actual completion time for attempts and HTTP-date scheduling.
Public payload/configuration review, secure credential/state handling, and runtime
activation remain pending. The executable still cannot monitor or notify.

## Current increment: private transport-supervision boundary

Added a test-only Linux/macOS child supervisor with bounded concurrent stdin/stdout,
cleared environment, discarded stderr, total monotonic deadline, process-group
cleanup, and strict successful-exit acknowledgment parsing. Framing checks total
size before allocation. Six supervisor tests and one worker/reopen test bring the
suite to 76 passing Rust tests, eight SQLite tests, and six hook tests. Formatting,
strict Clippy, locked build, and executable smoke checks pass. No dependencies,
public contracts, or product defaults changed. ADR 0004 records cleanup limits.

The actual HTTP adapter remains in-process. Next: connect it to a trusted private
child entry point with bounded frame decoding, test blocked DNS and real HTTP
results across the process boundary, and use attempt completion timestamps for
retry scheduling. Avoid presenting the supervisor fixture as working notifications
or a completed production deadline guarantee. Runtime activation and public
payload/configuration decisions remain pending.

## Current increment: supervised webhook child integration

Connected HTTP delivery to a private custom Cargo test child. The normal executable
is unchanged. Added bounded six-field decoding, invalid/truncated/trailing-input
rejection before sending, and sanitized outcome output. Two unit tests bring the
Rust suite to 78 passing tests; eight SQLite and six hook tests also pass. Seven
separate process-boundary acceptance scenarios cover acknowledgment, rate limiting,
rejection, slow HTTP timeout, TLS rejection, blocked resolution termination, and
malformed frames. Formatting, strict Clippy, locked build, and smoke checks pass.
No dependencies or public contracts changed. ADR 0004 records the test-only limits.

Next: use actual per-attempt completion times for durable retry scheduling and
HTTP-date Retry-After calculations, including slow sends and clock changes. Secure
installed helper selection, runtime credential/state handling, public payload/config
review, and platform cleanup validation remain pending before usable monitoring.

## Current increment: completion-time retry scheduling

The private clock-aware worker now samples each attempt after its provider returns,
records completion time, and schedules the full retry delay from that timestamp.
Clock failure/regression and retry-time overflow leave the uncommitted attempt
pending while preserving earlier commits. The webhook evaluates HTTP dates against
response completion time. The supervised child uses its actual Unix clock; private
requests no longer carry a stale batch timestamp. Existing fixtures retain explicit
fixed-clock wrappers, without exposing product defaults or a public contract.

Five new unit tests bring the suite to 83 passing Rust tests, eight SQLite tests,
and six hook tests. Eight process-boundary scenarios also pass, including a past
HTTP-date response with the child's actual clock. Formatting, strict Clippy, locked
build, and placeholder smoke checks pass. No dependencies or migration changed.

Next: bounded retry jitter and explicit transient/permanent transport diagnostics,
then draft normalized event payload/configuration proposals for review. Installed
helper selection, secure state/credential handling, runtime activation, and approved
platform support remain pending; the executable cannot monitor or notify yet.
