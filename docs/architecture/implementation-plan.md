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

## Current increment: bounded jitter and sanitized transport diagnostics

Added explicit deterministic jitter bounds/seeds, preserving backoff and provider
floors while spreading retries even at the backoff cap. Due times persist unchanged
across reopen; checked arithmetic rejects jitter overflow without committing an
attempt. Added typed HTTP/transport reports without request or response secrets;
permanent configuration error kinds are terminal, while transient categories retain
the existing retry budget. TLS errors remain generic I/O retries because the client
kind does not distinguish them reliably. Diagnostics are private and not persisted
or exposed through a CLI or the child protocol.

Six tests bring validation to 89 passing Rust tests, eight SQLite tests, and six
hook tests, plus eight process-boundary acceptance scenarios. Formatting, strict
Clippy, locked build, and executable smoke checks pass. No dependencies, schema,
public contracts, or operational defaults changed. The normal executable remains
unavailable for monitoring and notification delivery.

Next: draft normalized webhook payload and configuration proposals for review,
covering identity encoding, privacy, delivery semantics, and explicit defaults.
Installed-helper selection, credential/state security, diagnostics persistence,
platform support, licensing, and runtime activation remain pending.

## Current increment: webhook payload and configuration review drafts

Drafted concrete field/type semantics and synthetic examples under docs/proposals,
covering lifecycle mappings, opaque persisted IDs, unknown timestamps, frozen bytes,
minimal metadata, routing, secret references, resource controls, explicit values,
and compatibility. Proposed initial JSON configuration in place of the handoff's
illustrative YAML, explicitly reserving that format choice for review. SMTP remains
part of the handoff's MVP requirements rather than being removed by this scope.

Both example files parse as JSON; no machine-readable schemas, parser dependencies,
approved contracts, migrations, or executable behavior were introduced. The drafts
identify missing durable metadata, accounting overlap, route revision handling,
secure state creation, and installed-helper selection instead of implying readiness.
Local hooks still execute the full existing suite on commit/push.

Next: review the field/default/format/privacy decisions before exposing interfaces.
Independent private engineering can continue with secure state-path creation and
single-owner locking. Public serializers/config loaders, compatibility baselines,
SMTP, support/license decisions, and runtime activation remain pending.

## Current increment: private state permissions and single-owner locking

Added a private Linux/macOS owned-storage entry point: descriptor-based directory
walking, explicit private permissions, regular-file/owner/link checks, no-follow
opens, sidecar validation, and nonblocking lifetime advisory locking. Database
initialization failure and forced owner exit release the lock without deleting its
persistent inode. Existing low-level fixture storage is unchanged. ADR 0005 records
same-user trust, local-filesystem assumptions, symlink-free paths, and the distinction
between a private directory and the draft arbitrary database filename.

Six new tests bring the Rust suite to 95 passing tests; eight SQLite and six hook
tests, plus eight process-boundary acceptance scenarios, pass. Formatting, strict
Clippy, locked build, and smoke checks pass. Explicit test-only libc reuses the
already locked version; no new package, migration, public contract, or executable
activation occurred. Normal executable dependency tree remains empty.

Next independent work: integrate owned storage with the durable mock polling and
supervised delivery pipeline, then test restart/ownership failures together. Payload
and configuration decisions await review before public exposure. Licensing, SMTP,
installed-helper selection, supported platforms, and runtime activation remain open.

## Current increment: owned mock pipeline restart and delivery integration

Added two custom process acceptance scenarios that compose owned storage, durable
mock scheduler polling, restored reconciliation/checkpoints, the completion-clock
worker, and actual loopback HTTP from its supervised child. Verify 429 retry/reopen,
persisted due time, stable ID, checkpoint/deduplication restoration, no delivery
replay after acknowledgment, and competing-owner rejection before/after child use.
Also verify external acceptance followed by a failed local result commit: zero
partial attempts, pending outbox, same-ID replay after reopen, then successful commit.

Existing 95 Rust tests, eight SQLite tests, and six hook tests pass, with ten custom
process acceptance scenarios. Formatting, strict Clippy, locked build, and placeholder
smoke checks pass. No dependencies, migration, public contract, or runtime defaults
changed. Visibility changes only allow sibling test modules to compose private
components. The `{}` body and virtual completion times remain controlled fixtures.

Next: private accounting overlap/window planning and bounded restart catch-up,
including delayed accounting records. Public payload/configuration review, frozen
payloads/opaque IDs, safe installed-helper selection, SMTP, licensing, and actual
runtime activation remain pending before working monitoring can be claimed.

## Subsequent increment: private accounting window planning

A test-only planner derives UTC query boundaries from a durable checkpoint and
caller-supplied positive lookback/overlap values. Overlap must be smaller than
lookback. Initial queries use the bounded lookback; later queries begin at the
checkpoint minus overlap, clipped to the current lookback floor and Unix epoch.
Future checkpoints, malformed dates, unsupported times, and invalid policies fail
before querying. Repeating a checkpoint permits another overlapping query.

Separate flags identify downtime beyond the lookback and clipped overlap. They
are private diagnostic data, not proof of missing jobs. Accounting visibility
later than the overlap and records outside the bounded window may be missed;
Slurm selection semantics and site accounting latency remain unvalidated.

Four tests cover initial/restart windows, clipping, invalid inputs, clock
regression, UTC leap-day arithmetic, and delayed accounting after database reopen.
The composed polling test verifies failed queries preserve the checkpoint and
repeated delayed records produce one durable event/outbox entry. The full suite
passes: 99 Rust tests plus one ignored helper, ten custom process scenarios,
eight SQLite tests, and six hook tests. No dependency, runtime default, public
contract, or executable behavior changed.

Next: private bounded polling-cycle composition using this planner, including
visible catch-up gaps and cancellation between cycles. Public contract review,
SMTP, licensing, and operational activation remain pending.

## Subsequent increment: private bounded polling cycles

A caller-driven test-only controller composes durable state restoration, UTC
window planning, and transactional polling. Each step restores the committed
checkpoint and reconciler rather than retaining uncommitted candidate state.
Successful steps return event counts and window gap/clipping flags. Errors
retain their storage, window, or scheduler category. The caller supplies all
limits, destinations, initial event policy, timestamps, and cycle cadence.

The finite positive attempt budget is consumed before restoration/planning,
including failed cycles. Cancellation is checked before each cycle and does not
consume budget or touch storage. Exhaustion also prevents storage/query work.
Cancellation does not interrupt an in-flight query; the existing per-poll
scheduler deadline applies. This adds no sleep loop, background process,
persistent cycle budget, default polling interval, or operational CLI. The caller
must retain state ownership and obey cluster policy when integrating execution.

Five tests verify cancellation/exhaustion, failure budget consumption, restart
from the last committed checkpoint, duplicate suppression, visible downtime gaps,
clock regression, atomic rollback on checkpoint write failure, and corrupt-state
rejection before queries. The full shared suite passes: 104 Rust tests plus one
ignored helper, ten custom process scenarios, eight SQLite tests, and six hook
tests. No dependencies, migrations, public contracts, or runtime defaults changed.

Next: private retention planning for terminal snapshots and completed outbox
records, preserving deduplication and pending-delivery protection. Public contract
review, SMTP, licensing, cluster validation, and runtime activation remain pending.

## Subsequent increment: private retention candidate planning

A read-only storage query selects a bounded, deterministically ordered set of
completed destination deliveries for one cluster. The caller provides current
Unix time, a positive retention age, and row limit. The latest attempt must be
success or terminal failure and strictly older than the cutoff; pending rows,
missing attempt history, and budget-retired transient attempts are protected.
A transient attempt timestamp does not establish retirement completion time.

Candidates contain private event/destination IDs and recorded completion time.
They are advisory snapshots: any future deletion must revalidate status, age,
and attempt evidence in its write transaction. No records are deleted here.
Events, job snapshots, and checkpoints remain intact because restoration requires
job references and the event ledger prevents duplicate notifications and freezes
initial destinations. Removing terminal snapshots needs a tombstone/retention
horizon design; this increment does not bound total database growth or restored
ledger size. No migration, dependency, operational default, or public contract
changed.

Three tests cover bounds, ordering, cluster isolation, strict age cutoff, pending
and uncertain completion protection, unchanged outbox rows, invalid inputs,
epoch clipping, and stable plans/deduplication after reopen. The full suite passes:
107 Rust tests plus one ignored helper, ten custom process scenarios, eight SQLite
tests, and six hook tests.

Next: transactionally apply a bounded completed-delivery cleanup plan with stale
candidate revalidation and rollback/restart coverage, retaining deduplication
history. Terminal snapshot compaction, public contract review, SMTP, licensing,
cluster validation, and runtime activation remain pending.

## Subsequent increment: private transactional delivery cleanup

The test-only storage boundary now applies a caller-bounded candidate batch in
one immediate SQLite transaction. Candidates carry status and latest attempt
number as well as completion time. Every destination is rechecked for cluster,
completed status, latest terminal outcome, exact attempt/time/status agreement,
and strict retention age before its attempts and outbox row are removed. Missing
or changed candidates reject the whole batch. Invalid policies, oversized batches,
and duplicate IDs reject application. A previously applied plan is stale and
must be replanned rather than silently accepted.

Foreign-key deletion order is attempts then outbox; event/job history and
checkpoints remain intact. Existing event-ledger checks prevent cleaned
notifications or new destinations from being recreated during reconciliation.
This is private test behavior only: no runtime cleanup schedule, database-file
compaction, terminal snapshot removal, migration, dependency, or public contract
is introduced. Total ledger growth and restoration capacity remain unresolved.

Three additional tests cover bounded application, cluster/age rejection, stale
status/time/attempt changes, rollback after earlier candidates were deleted,
injected deletion failure, retry after reopen, preserved pending deliveries,
foreign-key consistency, and no notification/destination replay after cleanup.
The full suite passes: 110 Rust tests plus one ignored helper, ten custom process
scenarios, eight SQLite tests, and six hook tests.

Next: private storage health diagnostics for queue depth, protected retention
records, and restoration capacity pressure, using bounded aggregate queries and
sanitized reports. Terminal ledger compaction, public contract review, SMTP,
licensing, cluster validation, and runtime activation remain pending.

## Subsequent increment: private storage health snapshots

A test-only read transaction collects a constant-size sanitized health report for
one caller-selected cluster. Jobs and events are probed to capacity plus one,
providing explicit over-capacity flags without loading full restoration history.
Destination inspection stops at the caller's sample limit plus one, reporting
truncation separately from sampled pending/due/delivered/failed counts. Cleanup
eligibility uses the same latest terminal-attempt evidence and strict age cutoff
as retention planning. Completed rows lacking such evidence are counted separately.
Counts are samples when truncated, not exact global totals or an unbiased estimate.
An exact-capacity sample is not flagged as already exceeding capacity.

One read transaction keeps the probes consistent. Row limits bound returned and
inspected rows, not SQLite execution time or underlying query scan/sort work.
No hard database query deadline or large-history performance claim is made.
The report contains only counts and flags; invalid input and database failures
use static categories without raw errors, paths, identities, or destination data.
This is not an integrity check and does not guarantee restoration succeeds for
malformed records. No public status/doctor interface, operational default,
dependency, migration, or production runtime behavior changed.

Three tests cover queue/retention classification, strict age and epoch handling,
cleanup and reopen consistency, cluster isolation, privacy, sample truncation,
capacity overflow, invalid/overflowing limits, and sanitized database failure.
The shared suite passes: 113 Rust tests plus one ignored helper, ten custom process
scenarios, eight SQLite tests, and six hook tests.

Next: private poll-health tracking for consecutive scheduler failures and stale
successful checkpoints, composing storage counts without raw command output.
Terminal ledger compaction, public contract review, SMTP, licensing, cluster
validation, and runtime activation remain pending.

## Subsequent increment: private poll-health tracking

The private cycle controller tracks a saturating count of consecutive failed
cycles and static categories for active/accounting query, reconciliation, invalid
request, window, and storage failures. Only a successful durable commit clears
the streak. Cancellation and exhausted budgets neither clear nor increment it.
Categories contain no raw query output, paths, job identity, or provider data.
Counts include all attempted cycle failures, not only scheduler failures.

The composed report refreshes the durable checkpoint through bounded restoration,
even before the first cycle after restart, and adds the storage health sample.
Checkpoint age is elapsed time since the committed accounting-window end, not
provider delivery age or actual poll completion latency. A caller-supplied
positive threshold marks age greater than or equal to the threshold as stale;
missing checkpoints are stale with unknown age. Invalid policy and clock
regression are explicit static errors. Failure streaks are in memory and reset
on a new controller; no persistent health migration is implied. Callers must use
one controller per cluster. Restoration and storage reports use separate read
transactions and require the existing serialized owner for consistent composition.
No public status/doctor command, monitoring loop, default threshold, dependency,
or production executable behavior changed.

Three tests cover failure-source classification, exact staleness boundaries,
recovery, cancellation/exhaustion, durable restart checkpoint age, failed commits,
clock regression, invalid policy, counter saturation, sanitized reports, and
storage-report failure. The shared suite passes: 116 Rust tests plus one ignored
helper, ten custom process scenarios, eight SQLite tests, and six hook tests.

Next: private caller-driven orchestration of bounded poll cycles, due delivery,
cleanup, and health reporting, with failure isolation and cancellation boundaries.
Terminal ledger compaction, public contract review, SMTP, licensing, cluster
validation, and runtime activation remain pending.

## Subsequent increment: private orchestration ticks

A caller-driven test-only tick serializes bounded polling, due-delivery batches,
completed-delivery cleanup, and composed health reporting against one owned store.
It accepts explicit settings, limits, provider/query callbacks, a completion clock,
and cancellation callback; there is no cadence default or background loop.
Provider callbacks retain responsibility for their own bounded transport.

Scheduler query/reconciliation errors do not prevent delivery of previously
committed outbox records. Poll restoration/commit or window failures stop later
stages. An exhausted poll budget permits maintenance without additional queries.
Delivery errors stop cleanup and reporting because prior sends may be committed
or externally accepted but unrecorded. Cleanup failure remains visible and still
permits read-only reporting. Partial successful work remains committed; a tick
is not one transaction. Its optional stage results distinguish unrun stages from
successful empty work and use static error categories without raw diagnostics.

Cancellation is checked before the tick and between stages, never fabricating
provider outcomes or interrupting active queries, delivery batches, or cleanup
transactions. Later clock samples must not regress, including per-send completion
times. Cleanup and reporting sample fresh times after delivery. No production
CLI, public contract, dependency, migration, runtime default, or release changed.

Three tests cover retry/delivery after scheduler failure and database reopen,
cleanup with retained deduplication, cancellation before/between stages, accepted
send with failed commit, and clock regression before delivery. Existing component
rollback and transport deadline tests remain in the full suite. Validation passes:
119 Rust tests plus one ignored helper, ten custom process scenarios, eight SQLite
tests, and six hook tests.

Next: extend the owned-store/process-boundary acceptance harness to use composed
orchestration, verifying actual supervised HTTP retries and cancellation between
stages. Terminal ledger compaction, public contract review, SMTP, licensing,
cluster validation, and runtime activation remain pending.

## Subsequent increment: owned orchestration process acceptance

The custom process harness now imports the existing private planner and cycle
controller and exercises a composed orchestration sequence through the trusted
HTTP child. Cancellation after the poll commit leaves one pending notification
without sending. After reopening the owned store, a real loopback 429 response
persists a retry deadline from the virtual completion time plus Retry-After.
Another reopen retries with the same internal ID and attempt two, despite a
scheduler query failure, and receives HTTP 202. A later tick removes the completed
destination, retains event deduplication, and clears the recovered poll-health
failure streak. A final reopen confirms no pending replay. Competing ownership
is rejected while the store remains held after child execution.

The harness uses framed stdin, isolated ephemeral loopback endpoints, virtual
clock values, synthetic scheduler records, and the private `{}` payload fixture.
It validates orchestration and process boundaries, not real Slurm compatibility,
public payload semantics, endpoint revision freezing, or live completion clocks.
Only sibling test-module visibility changed; no production behavior, dependency,
migration, contract, or runtime defaults changed.

The full shared suite passes: 119 Rust tests plus one ignored helper, eleven
custom process acceptance scenarios, eight SQLite tests, and six hook tests.

Next: private orchestration acceptance for cleanup failure isolation and
cancellation after a completed delivery batch, ensuring committed deliveries
survive while maintenance remains retryable. Terminal ledger compaction, public
contract review, SMTP, licensing, cluster validation, and activation remain pending.

## Subsequent increment: maintenance failure and post-delivery cancellation acceptance

An additional owned-store process scenario sends one event to two ephemeral
loopback destinations through the supervised HTTP child. Cancellation after the
completed delivery batch skips cleanup/reporting while preserving both successful
attempts. After reopening the owner, a test trigger rejects deletion of the
second destination. The cleanup transaction restores both outbox rows and attempt
histories, while the tick still produces its read-only health report. Another
reopen and trigger removal allow maintenance retry to delete both completed
records without invoking the send callback. Event deduplication remains intact
and foreign-key checks pass.

This validates cancellation and failure isolation across actual child delivery
and database reopen. It uses synthetic scheduling, virtual time, and private
payload fixtures. The trigger and raw test database connection are deliberate
fault injection, not supported production storage access. No implementation,
dependency, schema, public contract, or executable behavior changed.

The full suite passes: 119 Rust tests plus one ignored helper, twelve custom
process acceptance scenarios, eight SQLite tests, and six hook tests.

Next: private orchestration preflight validation of all caller-supplied policies
and limits before poll or delivery side effects, with rejection and boundary tests.
Terminal ledger compaction, public contract review, SMTP, licensing, cluster
validation, and activation remain pending.

## Subsequent increment: private orchestration preflight

The tick validates caller settings and maintenance policies before storage reads,
queries, sends, or poll-budget consumption. Checks cover nonempty command paths,
user/cluster, timeout/output limit, restoration capacity, nonempty unique
destinations, accounting window policy, retry policy, maintenance row limits,
retention age, and staleness threshold. Numeric probes must be representable;
output limits must permit the runner's extra-byte overflow probe. Shared retry
and window validation functions prevent policy drift between preflight and their
component implementations.

Cancellation still takes precedence. Static invalid policies are rejected before
sampling the clock. A valid UTC planner time and nonoverflowing retry schedule
are then required before the poll. The report has a separate preflight result;
rejections leave all stage results absent, do not consume the attempt budget,
and do not increment the poll-failure streak. Later clock, provider delay, storage,
and query failures remain checked at their existing boundaries. Validation does
not establish executable availability, cluster policy, credential validity, or
an arbitrary maximum allowed operational resource allocation. Those remain
separate decisions. No dependencies, public contract, default, or production
executable behavior changed; private destination lists now require at least one
unique nonempty destination at the orchestration boundary.

Two tests cover 27 invalid static configurations with callbacks that must never
run, unchanged budget/streak/checkpoint, invalid/unsupported clock values, retry
schedule overflow, and small valid limits. The full suite passes: 121 Rust tests
plus one ignored helper, twelve custom process scenarios, eight SQLite tests,
and six hook tests.

Next: owned orchestration acceptance for supervised HTTP timeout and durable
retry recovery, ensuring later maintenance does not erase pending sends.
Terminal ledger compaction, public contract review, SMTP, licensing, cluster
validation, and activation remain pending.

## Subsequent increment: owned supervised timeout retry recovery

A process acceptance scenario deliberately delays the HTTP acknowledgment beyond
the parent supervisor deadline. The local server captures the request before
child termination; the fixture maps the exact parent timeout to a retry outcome,
not success. The worker persists attempt one and backoff from its virtual
completion time. Cleanup runs at a later virtual time when retry is already due
and still protects the pending record. Reopening owned storage preserves the
retry deadline, internal ID, and attempt count. A second supervised HTTP request
receives 202 as attempt two. Later cleanup and another reopen retain the event
ledger without a pending duplicate.

The initial timeout is uncertain external acceptance: stable request identity
allows provider idempotency, but cannot guarantee it. The test explicitly compares
both idempotency headers and synthetic bodies. A broad elapsed-time bound checks
parent return without claiming real-time guarantees. Virtual clock values and
synthetic Slurm records remain fixtures. Timeout-to-retry mapping is still supplied
by the test callback rather than a general supervisor/provider policy. No public
contract, runtime behavior, schema, or dependencies changed.

The full shared suite passes: 121 Rust tests plus one ignored helper, thirteen
custom process acceptance scenarios, eight SQLite tests, and six hook tests.

Next: private supervised-provider result classification, making parent timeout,
child protocol/configuration failures, and uncertainty diagnostics explicit
instead of relying on acceptance callback mappings. Terminal ledger compaction,
public contract review, SMTP, licensing, cluster validation, and activation remain
pending.

## Subsequent increment: private supervised-provider result classification

A private parent classifier preserves acknowledged delivered/rejected/retry
outcomes and valid Retry-After delays. Acceptance is accepted/rejected only for
corresponding acknowledgments; generic retry acknowledgments retain unknown
acceptance. Timeout, I/O, output overflow, child exit, malformed acknowledgment,
and cleanup failures produce retry with unknown acceptance and static diagnostic
categories. Negative retry hints are treated as malformed results and retried.
These errors cannot establish whether an external provider accepted the request.

Invalid supervisor input, spawn failure, and unsupported platforms block dispatch
with no provider outcome and not-attempted acceptance. Callers must report these
local failures without inventing an external attempt or failing the outbox record.
The current delivery callback requires a provider outcome, so blocked dispatch
still needs explicit worker integration. The classifier consumes supervisor
results, not child-side frame decoding failures. The timeout process acceptance
now uses classification instead of a hand-written retry mapping.

Three unit tests cover acknowledged outcomes, retry hints, local dispatch blocking,
and every post-spawn error category. The full suite passes: 124 Rust tests plus
one ignored helper, thirteen custom process scenarios, eight SQLite tests, and
six hook tests. No dependencies, migration, public contract, runtime default,
or normal executable behavior changed.

Next: private fallible provider callback integration so blocked local dispatch
stops a batch without recording a fabricated delivery attempt, preserving earlier
commits and retryable pending rows across restart. Terminal ledger compaction,
public contract review, SMTP, licensing, cluster validation, and activation remain
pending.

## Subsequent increment: private fallible delivery callbacks

The private worker now accepts provider callbacks returning either an outcome or
static blocked-dispatch diagnostic. A classifier report converts to this result
without inventing an outcome when dispatch was not attempted. Blocked callbacks
stop the batch before sampling completion time or writing an attempt. Previously
committed destinations stay committed; the blocked and later destinations retain
their due times, IDs, and attempt numbers. Recovery after correcting the local
problem retries those pending rows without resending completed destinations.

The existing infallible entry point delegates to the same worker implementation,
preserving current orchestration and acceptance behavior. Classified post-spawn
timeout still records a transient attempt with normal bounded retry semantics.
Only callbacks that establish no external dispatch should return blocked; uncertain
post-send failures must retain the retry classification. Callback errors are static
and do not include raw secrets or command errors. The worker still stops on
storage or completion-clock failure after external acceptance.

Three tests cover blocked first dispatch without clock/attempt writes, a later
block preserving earlier commits and pending identity across reopen, successful
recovery, classified timeout backoff, and provider-delay preservation. The full
shared suite passes: 127 Rust tests plus one ignored helper, thirteen custom
process scenarios, eight SQLite tests, and six hook tests. No dependencies,
schema, public contract, default, or normal executable behavior changed.

Next: private fallible orchestration callbacks and owned process acceptance for
blocked helper dispatch after a prior successful destination, with maintenance
stop and restart recovery. Terminal ledger compaction, public contract review,
SMTP, licensing, cluster validation, and activation remain pending.

## Subsequent increment: fallible orchestration and blocked helper recovery

The private tick now accepts fallible provider callbacks and delegates to the
fallible worker. Existing infallible callers wrap outcomes through the same tick
implementation, retaining preflight, clock, cancellation, and error boundaries.
Blocked dispatch produces an explicit delivery-stage category and stops cleanup
and health reporting. Successful earlier destination commits remain durable;
no completion clock or attempt is recorded for the blocked destination.

An owned process acceptance scenario commits the first destination through the
trusted HTTP child, then attempts to spawn a deliberately missing helper for the
second destination. The parent classifier reports spawn/not-attempted, stopping
the batch before the third destination. Completion-clock call counts, pending
attempt numbers, and absent maintenance results are verified. Reopening the store
and using the real child delivers the remaining two destinations as attempt one
with the same internal event ID, without resending the first. Maintenance cleans
only the older completed destination and the event ledger remains restorable.

The missing helper is an isolated test fixture, not an installed-helper selection
policy. Raw endpoint/auth data stays in framed stdin; diagnostics remain static.
No public contract, dependency, migration, runtime default, or production
executable behavior changed. The full suite passes: 127 Rust tests plus one
ignored helper, fourteen custom process scenarios, eight SQLite tests, and six
hook tests.

Next: preserve sanitized partial delivery batch progress alongside failure details
so orchestration can report earlier committed work when a later dispatch blocks.
Terminal ledger compaction, public contract review, SMTP, licensing, cluster
validation, and activation remain pending.

## Subsequent increment: sanitized partial delivery failure reports

The private detailed worker returns committed batch counts alongside a static
failure category, optional blocked-dispatch diagnostic, and unrecorded-outcome
uncertainty flag. Counts advance only after each storage commit, including budget
retirement. A callback block or pre-send failure has no uncertain send; failure
after a provider callback and before its outcome commit flags uncertainty. This
flag describes incomplete durable bookkeeping, conservatively covering retry or
rejection outcomes too; it does not establish external acceptance.

Orchestration retains this report when delivery fails and still stops later
maintenance. Existing worker wrappers preserve their previous error behavior.
The failure's Debug output omits its internal storage error cause and shows only
counts, static categories, and flags. No event/destination IDs, command paths,
payloads, or raw database diagnostics are added to reports. Prior successful
commits remain durable and the failed item is excluded from committed totals.

Two tests verify partial success followed by completion-clock failure and zero
progress on invalid policy. The owned blocked-helper process scenario additionally
asserts one committed delivery, spawn diagnostic, no uncertain send, and sanitized
output. The full suite passes: 129 Rust tests plus one ignored helper, fourteen
custom process scenarios, eight SQLite tests, and six hook tests. No dependencies,
migration, public contract, runtime default, or executable behavior changed.

Next: private progress-report coverage for transient retries, terminal failures,
and budget retirement before a later storage failure, ensuring reported counts
match durable state across reopen. Terminal ledger compaction, public contract
review, SMTP, licensing, cluster validation, and activation remain pending.

## Subsequent increment: mixed durable progress verification

Two tests verify detailed batch progress against actual reopened SQLite state.
The mixed case retires an exhausted destination without sending, commits a
transient retry and terminal rejection, then injects a failure after a later
provider acceptance. Reported failed/retrying counts match the committed rows;
the unrecorded accepted item remains pending at its original attempt number.
Persisted retry time and attempt numbering survive reopen, and removing the
fault allows only the remaining destinations to complete. Debug output does not
expose the injected raw database error text.

The second case commits one successful send before a retirement write fails.
Its report excludes the uncommitted retirement and marks no uncertain send for
that retirement. After reopen and fault removal, the exhausted destination is
retired without invoking a provider or completion clock. These tests exercise
failure/recovery semantics, not a new retention or delivery policy. No code
behavior, dependencies, schema, public contracts, or runtime defaults changed.

The full suite passes: 131 Rust tests plus one ignored helper, fourteen custom
process acceptance scenarios, eight SQLite tests, and six hook tests.

Next: private delivery cancellation checks between destinations, preserving partial
progress and pending rows without fabricating provider outcomes or attempts.
Terminal ledger compaction, public contract review, SMTP, licensing, cluster
validation, and activation remain pending.

## Subsequent increment: delivery cancellation between destinations

A private controlled worker accepts provider, completion-clock, and cancellation
callbacks as one explicit input. It checks cancellation before any storage read
and before each destination, including budget retirement. Cancellation returns
sanitized partial progress with a distinct category and no uncertain send.
Earlier commits remain durable, while unsent destinations retain their IDs, due
times, and attempt numbers. Existing detailed/infallible wrappers delegate with
cancellation disabled, preserving current orchestration and process acceptance.

A provider already in flight is not interrupted: its completion clock and outcome
commit run before the next cancellation check. This prevents cancellation from
abandoning bookkeeping after external acceptance. Existing clock/storage failures
still take precedence if that bookkeeping cannot finish. Finishing the last item
returns a completed batch; no remaining destination needs a cancellation check.
The caller remains responsible for bounded provider transport and cancellation
signal lifetime. No defaults, public interface, schema, or dependency changed.

Three tests cover cancellation before storage access, cancellation raised inside
a provider callback, durable success and pending identity after reopen, cancelled
empty batches, and retained provider-directed retry deadlines. The full suite
passes: 134 Rust tests plus one ignored helper, fourteen custom process scenarios,
eight SQLite tests, and six hook tests.

Next: wire controlled delivery cancellation into private orchestration and process
acceptance, verifying mid-batch cancellation after a real supervised HTTP response.
Terminal ledger compaction, public contract review, SMTP, licensing, cluster
validation, and activation remain pending.

## Subsequent increment: orchestrated mid-batch cancellation acceptance

Private orchestration now passes its cancellation callback into the controlled
worker. A cancelled batch marks the tick cancelled, preserves its detailed
committed progress, returns the distinct cancelled delivery category, and skips
cleanup/reporting. In-flight provider completion bookkeeping still finishes before
checking the next destination. All existing infallible and fallible tick callers
use this implementation; their cancellation callbacks now run between destinations
as well as between stages and must be safe for repeated calls.

The new owned process scenario raises cancellation after an actual supervised
HTTP acknowledgment for the first of three destinations. Its outcome is committed,
the report retains one delivered count without uncertain bookkeeping, and no later
send or maintenance stage runs. Reopening owned storage preserves the remaining
two destinations at attempt one. Successful real child delivery recovers both
with the same event ID without resending the first; cleanup and health reporting
resume and deduplication remains restorable. Existing post-batch cancellation
acceptance still verifies cancellation after the complete batch.

The full suite passes: 134 Rust tests plus one ignored helper, fifteen custom
process scenarios, eight SQLite tests, and six hook tests. No dependencies,
migration, public contract, default, or normal executable behavior changed.

Next: private orchestration preflight/recovery when the restored ledger exceeds
capacity, making pressure visible without discarding deduplication history or
sending from an inconsistent state. Terminal compaction, public contract review,
SMTP, licensing, cluster validation, and activation remain pending.

## Subsequent increment: restored-ledger capacity preflight

After static/time validation, orchestration reads the existing bounded storage
health snapshot before any scheduler query or send. Job or event history above
the caller's restoration capacity rejects preflight with a distinct capacity
category and sanitized pressure snapshot. Poll, delivery, cleanup, and normal
health stages remain unrun; budget, failure streak, checkpoints, and pending
outbox rows remain unchanged. Storage-health failure also rejects preflight with
a static storage category. The probe introduces no mutation or automatic growth
of configured limits.

Tests seed valid durable lifecycle events, reopen the store with a smaller limit,
and verify no callbacks or history deletion. An explicitly larger caller limit
allows restoration and delivery at the original attempt numbers. Another test
covers one job with multiple event states: event-ledger pressure is reported even
when the job count fits. Exact capacity is accepted. Capacity growth caused by a
new poll still uses existing atomic reconciliation/commit checks. Limits do not
bound SQLite wall time; serialized state ownership remains required.

The full shared suite passes: 136 Rust tests plus one ignored helper, fifteen
custom process scenarios, eight SQLite tests, and six hook tests. No dependencies,
migration, public contract, runtime default, or executable behavior changed.

Next: private preflight checkpoint validation before maintenance when polling is
exhausted, preventing delivery under a regressed/invalid accounting clock while
preserving pending rows. Terminal compaction, public contract review, SMTP,
licensing, cluster validation, and activation remain pending.

## Subsequent increment: checkpoint preflight with exhausted polling

After bounded capacity checks, orchestration now restores the ledger and plans
against its durable checkpoint before any stage runs. Restoration failure rejects
preflight with static storage diagnostics; malformed, unsupported, or future
checkpoints reject with static time diagnostics. This applies even when the
controller has exhausted its poll budget, closing the maintenance path that
previously skipped restoration and discovered bad checkpoints only during later
health reporting. Pending rows, history, checkpoints, budgets, and failure streaks
remain unchanged on rejection. Missing checkpoints retain initial-window behavior.

Two tests cover exhausted polling after database reopen with future, malformed,
invalid-calendar, and pre-epoch checkpoint fixtures, unchanged pending attempt
numbers, valid-checkpoint recovery without queries, and inconsistent event ID/state
restoration failure without provider calls. Fault repair is explicit test database
injection, not an operational automatic repair interface. Successful maintenance
still uses the original committed query checkpoint and bounded delivery rules.
Read transactions require the serialized owner; no hard database query deadline
or automatic corruption recovery is implied.

The full suite passes: 138 Rust tests plus one ignored helper, fifteen custom
process scenarios, eight SQLite tests, and six hook tests. No dependency, schema,
public contract, default, or normal executable behavior changed.

Next: review private orchestration invariants and consolidate reusable acceptance
fixtures before adding another transport, including missing-checkpoint and
preflight cancellation/ownership coverage. Terminal compaction, public contract
review, SMTP, licensing, cluster validation, and activation remain pending.

## Subsequent increment: orchestration boundary review and fixture consolidation

Five owned process scenarios now share explicit synthetic scheduler and maintenance
settings plus the fixture query callback. Destination sets remain supplied by each
scenario. These helpers are test policy inputs, not operational defaults or a
production configuration layer.

Added coverage verifies missing checkpoints allow exhausted-budget maintenance
without another scheduler query, while health reports unknown/stale poll age and
no checkpoint is invented. Cancellation takes precedence over invalid static
settings and unusable storage: no clock, query, provider, preflight read, budget,
or failure-streak update occurs. Owned process acceptance verifies cancellation
keeps the advisory owner lock held until store close and permits reopening after
close without checkpoint mutation.

Review confirms the current boundaries: static/time/capacity/restoration preflight
before side effects; serialized per-cluster ownership; providers outside database
transactions; partial commits retained on later errors; cancellation completes
in-flight bookkeeping; cleanup preserves event-ledger identity. Resource row limits
still do not bound SQLite execution time, and the test harness does not establish
real-cluster compatibility, release security, or public interface approval.

The full suite passes: 140 Rust tests plus one ignored helper, sixteen custom
process scenarios, eight SQLite tests, and six hook tests. No implementation
policy, dependency, migration, public contract, or executable behavior changed.

Next: draft the private SMTP transport boundary and failure/retry acceptance plan,
covering TLS/authentication, bounded execution, and secret-safe diagnostics before
selecting a compatible test-only implementation dependency. Terminal compaction,
public contract review, licensing, cluster validation, and activation remain pending.

## Subsequent increment: private SMTP boundary proposal

ADR 0006 proposes one explicit relay, recipient, and frozen message per private
SMTP destination, behind a separate bounded child request mode. It records required
TLS/auth ordering, phase-aware acceptance/retry/block semantics, sanitized diagnostics,
message validation, resource limits, and dependency assessment gates. RFC 5321 and
RFC 3207 primary references inform protocol semantics; local policies and public
contracts remain proposed. No dependency or SMTP client is implemented here.

The acceptance matrix covers TLS certificate rejection before AUTH, STARTTLS
capability refresh, final DATA acknowledgment, temporary/permanent replies,
uncertain replay, bounded slow/DNS behavior, owned restart, cancellation, and
per-recipient isolation. Existing generic acceptance diagnostics require explicit
SMTP extensions before integration. Stable Message-ID is correlation, not guaranteed
deduplication. No runtime defaults, schema, or public interfaces changed.

Next: private SMTP transaction-phase classification with synthetic response fixtures,
including authentication/TLS blocks and final acknowledgment uncertainty, before
selecting a client dependency. Existing implementation validation remains unchanged.

## Subsequent increment: synthetic private SMTP phase classification

A test-only session policy tracks greeting, initial EHLO, required STARTTLS,
verified TLS, secure EHLO, optional explicit authentication, envelope submission,
recipient acceptance, DATA initiation, final acknowledgment, and completion.
Only valid expected positive replies advance the transaction; no intermediate
reply reports delivery. Final DATA 250 reports relay acceptance and later QUIT or
connection failure cannot undo it. Terminal failures remain sticky, preserving
uncertainty and blocks on repeated observations.

Temporary replies retry; permanent submission rejection is terminal; authentication
and required TLS failures block before message submission. Missing/malformed final
acknowledgment retains unknown acceptance. Failure before DATA body submission is
classified as not submitted when the caller's phase evidence establishes it. The
classifier assumes its future client supplies truthful phase/TLS evidence; it is
not a wire parser, certificate validator, AUTH implementation, or network adapter.
It contains no raw replies, addresses, credentials, or bodies and selects no client
dependency. No implicit TLS implementation or public email contract is introduced.

Three tests cover authenticated/unauthenticated relay flows, TLS/secure-EHLO ordering,
final acknowledgment, QUIT failure after acceptance, temporary/permanent/unknown
final results, repeated terminal evidence, and authentication/TLS blocks. The full
suite passes: 143 Rust tests plus one ignored helper, sixteen custom process
scenarios, eight SQLite tests, and six hook tests. No dependencies, schema,
operational defaults, or executable behavior changed.

Next: private bounded SMTP reply parsing with multiline, malformed/truncated, and
reply-size limit fixtures before selecting a client dependency. Public contract
review, persistence compaction, licensing, cluster validation, and activation remain
pending.

## Subsequent increment: bounded private SMTP reply reader

A test-only reader consumes exactly one reply through caller-supplied line-byte,
aggregate-byte, and line-count limits. It requires CRLF, a syntactically valid
reply code, consistent codes across multiline replies, and continuation/final
separators. Bare final codes are accepted. Its private ASCII text scope rejects
non-ASCII and control bytes except tabs in text; broader server compatibility
remains unvalidated. Following reply bytes remain unread. Only code, byte count,
and line count are returned; raw reply text is discarded and never appears in
static parse/I/O errors.

The implementation reads bounded individual bytes to avoid unbounded line buffer
allocation and hidden read-ahead. Limits include CRLF. Truncation, malformed data,
and resource exhaustion are distinct; exhausting the budget before a terminator
returns a limit error. Interrupted reads retry. Byte limits do not bound a blocked
or repeatedly interrupted reader; socket and parent deadlines remain mandatory
for network integration. No SMTP client, AUTH, TLS verification, or public wire
contract is introduced.

Four tests cover multiline/unread-next behavior, bare codes, malformed/truncated
input, exact boundaries, line/total/count limits, invalid limits, static I/O errors,
and interrupted reads. The full suite passes: 147 Rust tests plus one ignored
helper, sixteen custom process scenarios, eight SQLite tests, and six hook tests.
No dependencies, migration, runtime defaults, or executable behavior changed.

Next: private loopback SMTP dialogue fixtures composing reply parsing and phase
classification, including slow/truncated final acknowledgments, before client
and TLS dependency assessment. Public contracts, persistence compaction, licensing,
cluster validation, and activation remain pending.

## Subsequent increment: loopback SMTP envelope dialogue fixtures

Two tests compose the bounded reply reader with the private phase classifier over
an ephemeral loopback connection. A bounded fixture server verifies exact envelope,
recipient, DATA, and synthetic body writes, including multiline recipient replies.
Parsed final 250/451/550 responses produce relay acceptance, retry, or permanent
rejection respectively. Truncated, inconsistent multiline, and delayed final
acknowledgments preserve unknown acceptance and retry policy.

The classifier is prepared using synthetic TLS evidence before the envelope-only
connection. No real greeting/EHLO/STARTTLS handshake, AUTH, certificate validation,
message builder, or installed SMTP client is claimed. Plaintext is confined to
these unauthenticated parser fixtures. Both client/server have socket timeouts;
server acceptance has a bounded wait and command reads have exact finite sizes.
The delayed fixture verifies socket-read timeout behavior, not an absolute parent
deadline or drip-fed-reply deadline. Future supervised execution remains required.
All addresses and body bytes are synthetic; assertion failures do not echo them.

The full suite passes: 149 Rust tests plus one ignored helper, sixteen custom
process scenarios, eight SQLite tests, and six hook tests. No dependencies,
migration, public contract, defaults, or normal executable behavior changed.

Next: assess a compatible test-only SMTP/message dependency against ADR 0006,
including enforced TLS, authentication ordering, parser resource bounds, and
Rust 1.74 compatibility, before wiring a real private SMTP adapter. Public contracts,
persistence compaction, licensing, cluster validation, and activation remain pending.

## Subsequent increment: SMTP dependency gate assessment

ADR 0007 records exact tagged lettre source findings. Version 0.11.19 declares
Rust 1.74 but has no synchronous reply byte cap; 0.11.23 declares Rust 1.85 and
checks sizes only after line reads allocate. Neither transport is selected.
TLS/authentication builder capabilities are documented without claiming runtime
acceptance. Transitive builds and message-builder behavior remain unverified.
No dependency, toolchain, public contract, or executable changes were made.

Next: private implicit-TLS greeting/EHLO fixtures with the existing rustls test
dependency and bounded reply reader, verifying trust, hostname, and expiry failures
before authentication or envelope submission. STARTTLS, supervised deadlines,
message construction, public contracts, licensing, and activation remain pending.

## Subsequent increment: private implicit-TLS greeting boundary

A test-only rustls exchange explicitly finishes certificate-verified TLS before
reading the bounded greeting or writing EHLO. It accepts only greeting 220 and
EHLO 250, uses the existing bounded multiline reply reader, and returns static
TLS, I/O, or reply errors without server text. It contains no AUTH or submission
path. Fixture trust roots and ephemeral certificate keys remain in test code.

Three tests verify trusted multiline EHLO, unknown issuer/wrong hostname/expiry
rejection with zero observed SMTP command bytes, invalid greetings with no EHLO,
and inconsistent EHLO replies with no subsequent commands. The fixture observes
bounded command bytes and reports only counts and an exact synthetic EHLO match.
Accept waits, connect, handshake I/O, and socket reads/writes have finite timeouts.
Accepted sockets explicitly use blocking mode for cross-platform behavior.
Socket inactivity timeouts do not establish an absolute drip-fed peer deadline;
that remains a supervised-process acceptance requirement. TLS errors currently
combine certificate and handshake I/O failures; future worker classification must
distinguish policy blocks from transient network failure before integration.

No dependencies, runtime defaults, persistence, or public interfaces changed.
The full suite passes: 152 Rust tests plus one ignored helper, sixteen custom
process scenarios, eight SQLite tests, and six hook tests.

Next: private required-STARTTLS negotiation fixtures proving missing/refused
upgrade stops before credentials/envelope, successful verified upgrade obtains a
fresh EHLO, and certificate failures stop after upgrade. AUTH, message construction,
supervised absolute deadlines, restart integration, public review, licensing,
cluster validation, and activation remain pending.

## Subsequent increment: private required-STARTTLS negotiation

The private exchange reads bounded greeting/EHLO replies, requires an exact
case-insensitive STARTTLS extension on a capability line after the server's first
EHLO line, and accepts only upgrade reply 220. A fresh EHLO is sent only after
certificate-verified TLS completes. Plaintext capabilities are not retained.
There is no downgrade, AUTH, or envelope path. Reply parsing now offers a private
callback borrowing validated line text for immediate capability inspection;
returned replies and static errors still retain no server text. Callback evidence
is provisional until the entire reply parses successfully.

Three new tests cover absent/misleading advertisements, temporary/permanent upgrade
refusal, successful lowercase advertisement and verified fresh EHLO, unknown issuer,
wrong hostname, expired certificate, and failed secure EHLO. Fixtures observe exact
bounded synthetic commands and assert no further command after each outcome.
No external relay, dependency, public interface, or executable changes were made.
The full suite passes: 155 Rust tests plus one ignored helper, sixteen custom
process scenarios, eight SQLite tests, and six hook tests.

Socket timeouts remain inactivity bounds, not an absolute parent deadline. TLS
errors still combine verification and handshake I/O; upgrade refusal is a static
experiment result rather than an approved worker retry classification. Credentials
and message submission are absent, so these tests establish negotiation ordering,
not acceptance of an authenticated mail transaction.

Next: supervise a separate private SMTP negotiation child and prove absolute
parent deadlines for stalled/drip-fed greeting and upgrade replies. Then distinguish
certificate policy blocks from transient handshake failures before AUTH/message
integration. Message construction, durable email persistence, public review,
licensing, cluster validation, and activation remain pending.

## Subsequent increment: supervised private SMTP negotiation deadlines

The existing custom harness now has a separate negotiation-only child mode,
using the private required-STARTTLS exchange and existing Unix supervisor. Its
single-field length-prefixed stdin frame accepts only a nonzero decimal loopback
port (at most five bytes), validates EOF before connecting, and retains empty
trust roots. It inherits the supervisor's cleared environment and bounded pipes.
It is fixture-only, has no credentials or submission, and never emits a delivery
acknowledgment. Negotiation exits are not mapped into worker SMTP outcomes yet.

Four loopback scenarios stall or drip-feed greeting and upgrade replies. Drip
bytes arrive every 20 ms without completing a line, below parser byte limits and
well inside the child's two-second socket inactivity timeout. Parent timeout is
500 ms; tests require the timeout category and return within 1.5 seconds including
cleanup. Servers confirm closure, at least two drip writes, and exact EHLO/STARTTLS
commands before stalled upgrade responses. Accept/connect/read/write waits and
fixture writes are finite. Two controls cover complete upgrade refusal without a
delivery acknowledgment and oversized frame-prefix rejection before network use.

The full suite passes: 155 Rust tests plus one ignored helper, twenty-two custom
process scenarios, eight SQLite tests, and six hook tests. No dependency, runtime,
public contract, or persistence change was made. These tests bound the plaintext
negotiation waits; TLS handshake, secure EHLO, DNS, AUTH, and DATA-specific SMTP
process acceptance remain pending. Existing supervision cleanup may add latency
after the request deadline, so this is not a hard real-time execution guarantee.

Next: distinguish TLS certificate policy blocks from transient handshake I/O
failures and exercise the TLS handshake boundary under supervision before
credentials or message submission. Message construction, durable email state,
public review, licensing, cluster validation, and activation remain pending.

## Subsequent increment: TLS handshake classification and supervision

Both private TLS exchanges inspect typed causes from rustls handshake I/O errors.
Invalid or missing certificates return a static certificate category; other typed
rustls failures return protocol, ordinary I/O failures return handshake I/O, and
client-connection construction errors return local configuration. No error text
or inner cause is retained. Existing real certificate fixtures now require the
certificate category. One new unit test verifies certificate/protocol causes and
ordinary timeout, EOF, and reset categories, including diagnostic-text exclusion.
These are diagnostic categories, not approved worker retry/block mappings.

Three additional process scenarios complete plaintext STARTTLS negotiation and
observe a bounded TLS ClientHello record before a stalled handshake, drip-fed
partial handshake record, or immediate disconnect. The first two return parent
timeout at 500 ms, within the 1.5-second cleanup allowance, despite a two-second
socket inactivity timeout. Drip bytes arrive every 20 ms and at least two writes
are observed. Disconnect returns child failure promptly without a delivery ACK.
Server accept/socket waits and record allocations remain bounded. No secure EHLO,
AUTH, or envelope bytes can follow an incomplete handshake in these fixtures.
The fixture-only child still discards specific categories at process exit;
propagating them safely to the delivery worker remains a separate milestone.

The full suite passes: 156 Rust tests plus one ignored helper, twenty-five custom
process scenarios, eight SQLite tests, and six hook tests. No dependencies,
persistence, runtime defaults, or public interfaces changed.

Next: define a private SMTP negotiation diagnostic acknowledgment distinct from
relay acceptance, preserving certificate policy and transient handshake evidence
across the supervised child boundary. Then test blocked versus retry bookkeeping
before implementing AUTH or message submission. Message construction, public
review, licensing, cluster validation, and activation remain pending.

## Subsequent increment: private SMTP negotiation acknowledgment

The supervisor now accepts a private typed decoder while its existing HTTP wrapper
retains the same provider-outcome decoder. Deadline, pipe limits, cleared environment,
exit-status validation, and process cleanup remain shared. SMTP uses exact static
SMTP_NEGOTIATION tokens for READY, INPUT, CERTIFICATE, CONFIGURATION, IO, PROTOCOL,
REPLY, and UPGRADE. Every token fits the 32-byte output limit. READY is only a
negotiation diagnostic and has no conversion to relay acceptance or Delivered.
Only successful child exit plus complete canonical output establishes a diagnostic.

The loopback-only child writes sanitized diagnostics after negotiation or local
validation. Input size/EOF validation and empty trust roots remain in force. Complete
upgrade refusal and malformed input now preserve their categories rather than
collapsing into child exit; handshake disconnect preserves IO. Real self-signed TLS
rejection produces CERTIFICATE with no secure SMTP command, and a malformed TLS
handshake produces PROTOCOL. Two hostile child controls verify HTTP delivery tokens
are invalid under the SMTP decoder and a valid certificate token is ignored after
nonzero exit. Existing parent deadline fixtures continue returning timeout.

Two unit tests cover all eight canonical tokens and reject delivery tokens,
truncated acknowledgments, CRLF variants, trailing text, raw-detail additions, and
invalid UTF-8. The full suite passes: 158 Rust tests plus one ignored helper,
twenty-nine custom process scenarios, eight SQLite tests, and six hook tests.
No dependencies, runtime defaults, persistence, or public interfaces changed.

Next: private parent policy and durable blocked-versus-retry bookkeeping using the
negotiation evidence, proving pending rows survive certificate/input blocks while
transient errors schedule bounded retries. READY must remain excluded from delivery
success. AUTH, message construction/submission, public review, licensing, cluster
validation, and activation remain pending.

## Subsequent increment: SMTP negotiation dispatch and durable retry policy

A private classifier maps acknowledged negotiation evidence into the existing
fallible worker. INPUT, CERTIFICATE, CONFIGURATION, and UPGRADE block dispatch;
READY also blocks because successful negotiation is not completed delivery.
Acknowledged IO, PROTOCOL, and REPLY produce bounded retry. Every acknowledged
negotiation result carries explicit NotSubmitted evidence, distinct from the
existing NotAttempted and Unknown categories. Parent supervisor errors retain
existing conservative local-block/transient-retry behavior; lost acknowledgment
cannot prove child phase. No path produces Delivered or permanent rejection.

UPGRADE remains a conservative block because its current token conflates missing
extension and temporary/permanent refusal. REPLY retries without reply-code/phase
precision. These provisional policies must be refined before real submission;
they are not claims about authenticated SMTP transaction acceptance.

Three policy tests cover all acknowledged categories, READY exclusion, and parent
spawn/timeout/failed-child/invalid-result/cleanup behavior. Two injected durable
tests verify every block preserves pending event/destination/attempt after reopen,
with zero attempt rows and no completion-clock sampling, and each transient
category persists completion-based deadlines, advances attempt numbers, preserves
identity after reopen, and stops dispatch after the three-attempt fixture budget.

The full suite passes: 163 Rust tests plus one ignored helper, twenty-nine custom
process scenarios, eight SQLite tests, and six hook tests. No schema, dependency,
runtime default, or public contract changed. These injected tests compose policy
with storage; process acknowledgment acceptance is covered separately.

Next: compose actual supervised SMTP certificate blocks and transient failures
with owned storage and reopen, including prior-destination progress, before adding
credential or message paths. Refusal code/phase precision, AUTH, message construction,
public review, licensing, cluster validation, and activation remain pending.

## Subsequent increment: owned SMTP partial-progress restart acceptance

Two custom process scenarios seed a durable synthetic poll into an owned store
with three destinations. The first destination receives either a real supervised
handshake-disconnect IO acknowledgment or a parent timeout; the next destination
receives a real unknown-issuer certificate acknowledgment. The worker commits the
first retry and stops on the certificate block before the third destination.
Only the transient result samples completion time. Partial failure reports retain
one retry, zero delivered/failed results, and no unrecorded-send uncertainty.

Reopen preserves event identity, the first destination's attempt two and due time,
and attempt one for blocked/untouched destinations. A competing owner remains
rejected. Repeating the actual certificate block after reopen adds no attempts,
preserves the prior retry, and leaves the untouched destination pending. Database
checks require exactly one attempt and zero delivered rows; ledger restoration
retains the original emitted event. No credential, message, or acceptance claim is
introduced. The shared bounded handshake fixture and poll helper now serve both
isolated and composed scenarios without changing their previous assertions.

The full suite passes: 163 Rust tests plus one ignored helper, thirty-one custom
process scenarios, eight SQLite tests, and six hook tests. No schema, dependency,
runtime default, or public contract changed.

Next: preserve STARTTLS missing-extension versus temporary/permanent refusal
precision through private acknowledgments and policy before AUTH, so a temporary
upgrade refusal does not remain a configuration block. Then assess bounded
credential framing and verified-TLS authentication fixtures. Message construction,
public review, licensing, cluster validation, and activation remain pending.

## Subsequent increment: precise required-STARTTLS refusal policy

Missing EHLO extension, temporary 4xx upgrade refusal, and permanent 5xx refusal
now produce distinct private errors, acknowledgments, and parent diagnostics.
Only upgrade 220 starts TLS. Unexpected upgrade replies return reply failure;
non-250 EHLO is also reply failure rather than missing-extension evidence.
Temporary refusal retries through existing completion-based backoff; missing and
permanent refusal block without recording an attempt or failing the pending row.
All acknowledged results still carry NotSubmitted evidence. READY remains blocked.

The ambiguous UPGRADE token is removed and explicitly rejected. Replacement
TLS_MISSING/TLS_TEMP/TLS_PERM tokens fit the existing 32-byte output bound and carry
no server text. Existing TLS, codec, policy, pending-row, and retry-budget matrices
now cover the precise categories. Process refusal fixtures verify temporary,
permanent, unexpected, and missing-extension replies, including closure before TLS
or subsequent commands. Three owned-store scenarios verify actual child results
produce one completion-clock retry/attempt for temporary refusal and zero attempts
or completion-clock samples for missing/permanent cases. Reopen preserves identity,
the temporary retry deadline and attempt two, and attempt one for blocks; none
marks delivery success.

The full suite passes: 163 Rust tests plus one ignored helper, thirty-seven custom
process scenarios, eight SQLite tests, and six hook tests. No dependency, schema,
runtime defaults, or public interface changed. Broad greeting/EHLO reply precision
and full authenticated transaction policy remain unfinished.

Next: bounded private credential inputs and verified-TLS AUTH PLAIN fixtures,
ensuring missing/invalid credentials block before connection, unsupported secure
capabilities never trigger fallback, and secrets stay out of diagnostics. Message
construction/submission, public review, licensing, cluster validation, and activation
remain pending.

## Subsequent increment: bounded verified-TLS AUTH PLAIN experiment

A private credential type validates nonempty printable ASCII username/password
before invoking an injected connector, rejecting controls, NUL, non-ASCII, missing
values, and username/password lengths above explicit fixture ceilings of 64/128
bytes. It has no Debug, Clone, or credential accessors. The empty authorization
identity and explicit authentication identity/password form a bounded SASL PLAIN
payload. Owned payload and encoded command buffers use existing zeroize support,
reserve bounded capacity, and are dropped immediately after the AUTH write.
Caller-owned inputs and TLS internal copies are outside that zeroization guarantee.
Unicode/SASLprep and other credential-source compatibility remain unvalidated.

The shared TLS helper now returns a verified STARTTLS stream; existing negotiation
behavior and fixtures remain intact. Authentication obtains a fresh secure EHLO
and requires a case-insensitive exact AUTH extension advertising PLAIN on a
capability line. Pre-TLS capabilities are discarded. Unsupported mechanisms do
not trigger fallback. One AUTH command is sent over verified TLS: 235 succeeds
only as authentication, 4xx is temporary, 5xx is permanent, and challenges or
unexpected replies fail without internal retry. Teardown cannot replace an explicit
AUTH result. No branch submits a message or produces Delivered.

Three tests cover validation/no-connect behavior, exact command encoding/size,
trusted one-command authentication and reply classes, no fallback or stale capability
use, inconsistent secure EHLO, and unknown issuer/wrong hostname/expiry rejection
with zero observed AUTH commands. Fixture command buffers are bounded and zeroized;
boolean assertions avoid credential-byte output. Accept/connect/socket waits remain
finite; supervised AUTH deadlines and cross-process credentials are still pending.

Base64 0.22.1 is now a pinned direct dev dependency, reusing the existing locked
package for standard encoding without adding a package version. The lockfile changes
only the local crate dependency list. The full suite passes: 166 Rust tests plus
one ignored helper, thirty-seven custom process scenarios, eight SQLite tests, and
six hook tests. No runtime defaults, schema, or public contract changed.

Next: separate bounded private authenticated-SMTP child request framing and sanitized
AUTH acknowledgments, then supervised slow/disconnected AUTH response fixtures.
Credential lookup, worker outcome integration, message construction/submission,
public review, licensing, cluster validation, and activation remain pending.

## User-requested increment: CI badges and 80% coverage gate

CI now checks the shared suite and rustdoc on Linux/macOS, validates workflows
with pinned Actionlint, and reviews PR dependency vulnerabilities. A separate job
runs pinned cargo-llvm-cov with the development compiler's matching LLVM tools,
exports JSON/LCOV/HTML, and fails below 80% aggregate workspace line coverage.
Actions and tools are pinned; least-privilege permissions and checksum verification
remain enabled. Weekly Dependabot updates cover Cargo and GitHub Actions. Main
pushes build and smoke-check a checksummed development placeholder artifact only
after platform, coverage, and workflow checks pass. No releases are published.

The local measured baseline is 95.87% Rust lines (8,165/8,517). Scope and limitations,
including embedded unit fixtures, default harness-file filters, and separate Python
checks, are documented. No coverage exclusions were added. The README badge labels
the coverage requirement and links to the policy. Coverage remains mandatory in CI
and optional in local hooks; branch-protection settings require separate repository
administrator configuration. Local coverage, Actionlint, and shared suites were
validated; hosted-run status is recorded when available after push.

Next implementation milestone remains authenticated SMTP child framing and
supervised AUTH deadlines. Runtime defaults, schema, public contracts, licensing,
cluster validation, and activation remain pending.

Hosted validation of the first CI push exposed unsupported tool installation in
the older installer pin. The installer was updated to a verified current immutable
commit for cargo-llvm-cov; Actionlint uses its fixed release archive and explicit
SHA-256 instead of unsupported installer fallback. Platform checks passed in that
run. Hosted checks are rerun after the installation correction.

## Subsequent increment: private authentication framing primitives

A separate three-field prototype frames loopback port, username, and password with
length prefixes and explicit 5/64/128-byte ceilings (209 bytes including prefixes).
The decoder checks lengths before allocation, requires exact EOF, validates a
nonzero numeric u16 port and existing credential policy, and returns a request
without Debug/Clone or raw credential accessors. Owned input fields and outgoing
frames use zeroizing buffers; caller strings and internal TLS copies remain outside
that guarantee. No credential source, arguments, or environment lookup was added.

Authentication-only static acknowledgments use a distinct SMTP_AUTH prefix. Exact
decoding rejects delivery/negotiation tokens, missing newlines, trailing bytes, and
raw diagnostic text. Authenticated is authentication evidence only; this module
has no delivery outcome conversion. Three tests cover maximum-size roundtrip,
invalid credentials/ports, every truncated prefix/body, oversized length prefixes,
extra input, and strict acknowledgment isolation. These are framing primitives,
not an operational child protocol. No dependencies or public contracts changed.

Next: preserve zeroizing frame ownership through the supervised writer, wire a
separate authenticated fixture child with verified test trust, and test slow,
drip-fed, and disconnected AUTH replies under the parent deadline. Worker outcome
integration, credential lookup, message submission, licensing, and activation
remain pending.

Validation: shared suite passes (169 Rust tests plus one ignored helper, 37 custom
process scenarios, eight SQLite tests, six hook tests), including formatting and
strict Clippy. The coverage gate passes at 95.90% Rust lines (8,300/8,655).

## Subsequent increment: supervised private authentication fixture child

The custom process harness now has a separate authentication-only child mode.
A bounded fixture envelope carries an ephemeral DER root (1–4,096 bytes) ahead of
existing credentials; aggregate input is capped at 4,309 bytes. This harness-only
trust provision is not an operational configuration or certificate bypass. Strict
frame/EOF and root validation finish before a hardcoded loopback connection;
certificate verification checks the fixed relay.invalid name. Connect/socket waits
remain two seconds, independently bounded by a 500 ms parent request deadline.

The generic supervisor now accepts owned AsRef byte buffers, retaining zeroizing
owners through the writer and dropping them before reporting write completion.
Existing Vec callers keep their behavior, environment clearing, process-group
cleanup, limits, and zero-exit requirement. Credentials remain absent from argv,
environment and static diagnostic output. Typed credentials pass directly to AUTH
without converting back to caller-owned strings. Order remains STARTTLS, verified
handshake, fresh secure EHLO, one AUTH; no MAIL/DATA or delivery acceptance occurs.

Authentication acknowledgments now preserve TLS certificate/configuration/protocol
and upgrade categories under the distinct SMTP_AUTH prefix. Truncated or I/O
replies classify as I/O; malformed/oversized protocol replies remain Reply. Twelve
new custom process scenarios cover 235, 454, 535, malformed reply, abrupt EOF,
stalled and drip-fed AUTH, malformed envelope lengths, trailing frame bytes, and
invalid DER. Stall/drip fixtures observe socket closure, drip proves continuing
traffic, and parent return plus cleanup stays below 1.5 seconds. Preflight controls
observe no loopback connection. Existing certificate and capability tests still
cover zero AUTH on verification failure and no unsupported-mechanism fallback.

No dependency, schema, normal runtime behavior, or public interface changed.
Next: classify authenticated-child outcomes for owned outbox retry/block handling,
retaining authentication-only success as not submitted, then verify restart and
partial progress. Credential lookup and message construction/submission remain
pending alongside public review, licensing, cluster validation, and activation.

Validation: formatting, strict Clippy and shared suite pass (169 Rust tests plus
one ignored helper, 49 custom process scenarios, eight SQLite tests, six hook
tests). Rust line coverage passes the 80% gate at 95.93% (8,320/8,673).

## Subsequent increment: authenticated-child outbox policy

A private parent classifier consumes the separate authentication acknowledgment.
Temporary AUTH, I/O, protocol/reply, and temporary upgrade failures retry; invalid
credentials/input, unsupported mechanisms, permanent AUTH, certificate/configuration,
and missing/permanent upgrade failures block. Static AUTH diagnostics distinguish
these outcomes. Authentication success remains not submitted and leaves the row
pending; no acknowledgment can produce Delivered or recipient rejection. Missing
child acknowledgments retain the supervisor's conservative unknown-acceptance policy.

Three classifier tests cover all acknowledged categories and representative parent
failures. Two durable unit tests exercise every blocking category, retained identity,
zero attempts/completion samples, and transient retry deadlines across reopen through
the three-attempt budget. Two custom owned-store scenarios compose actual child
454 or timeout results followed by 535: the earlier completion-time retry survives
the block, the later destination remains untouched, repeated permanent blocks add
no attempts, and ownership/identity/retry deadlines persist on reopen. No message
is marked delivered. Existing loopback fixtures are shared without changing their
bounded, verified-TLS behavior. No dependencies, schema, public contract, or runtime
default changed.

Next: bounded private SMTP message/envelope construction and submission fixtures,
with final DATA acknowledgment as the sole delivery-success evidence. Credential
lookup, public interface review, licensing, cluster validation and activation remain
pending.

Validation: shared checks pass (174 Rust tests plus one ignored helper, 51 custom
process scenarios, eight SQLite tests, six hook tests), including formatting and
strict Clippy. Coverage passes the 80% gate at 95.98% Rust lines (8,504/8,860).

## Subsequent increment: bounded private SMTP message construction

A private single-recipient builder uses lettre's independently assessed builder
feature; transport remains deferred. ASCII addr-specs are preflight bounded and
library validated. Stable identity and explicit frozen Date prevent regeneration
on repeated construction. The prototype rejects structured CR/LF/NUL injection,
unsupported mailbox syntax, non-ASCII fields, oversize subjects/body/identity,
and out-of-range dates before building. Text accepts printable ASCII and LF only;
lettre handles headers, content type/transfer encoding, and CRLF normalization.

The separate DATA framing helper checks canonical CRLF, 998-byte content lines,
ASCII and formatted length, doubles every leading dot (including a line containing
only a dot), and appends exactly one terminator within a separate byte ceiling.
Three tests verify deterministic bytes and envelope, terminal/newline/empty bodies,
dot transparency, field injection, library address rejection, every size boundary,
malformed formatted lines, and bounded DATA expansion. Owned formatted/DATA buffers
use zeroization; upstream/caller copies do not share that guarantee. No Debug or
Clone is provided for Prepared; errors contain only static categories.

Dependency features, compiler-compatible transitive pins, source assessment and
limitations are recorded in ADR 0007. No runtime defaults, schema, or public
contracts changed. This milestone constructs synthetic messages only.
Next: private submission using validated envelope/DATA, with phase-specific reply
policy and positive final DATA acknowledgment required for success. Supervised
message framing, durable frozen-message persistence, credential lookup, public
review, licensing, cluster validation and activation remain pending.

Validation: shared checks pass (177 Rust tests plus one ignored helper, 51 custom
process scenarios, eight SQLite tests, six hook tests), including formatting and
strict Clippy with the new locked builder dependencies. The 80% coverage gate
passes at 96.03% Rust lines (8,717/9,077).

## Subsequent increment: private authenticated SMTP submission

Verified STARTTLS plus fresh secure EHLO and successful AUTH can now return an
opaque authenticated stream. Only the authentication module constructs this type;
submission accepts it and a privately validated single-recipient message. Existing
authentication-only child behavior remains intact. Structured message input is
validated independently by the constructor; future child framing must finish all
input validation before connecting. No plaintext public submission
entry point, credential lookup, new dependency, schema, or normal runtime behavior
was introduced.

The private transaction sends MAIL, one RCPT, DATA, and the already bounded/dot-stuffed
message, reading each reply through the existing bounded parser (128 bytes per
line, 256 total, four lines). Exact expected success codes advance the phase; 4xx
retry and 5xx reject. Unexpected/malformed replies and I/O retry without continuing
the transaction. Pre-DATA failure is not submitted; explicit final DATA refusal is
not accepted; missing/malformed final acknowledgment or DATA write failure carries
unknown acceptance. Only final DATA 250 yields Delivered as relay acceptance.
Best-effort QUIT writes do not replace that result; no QUIT reply is awaited.
Socket timeouts remain caller responsibilities until child integration adds the
absolute parent deadline. A killed child before acknowledgment can still replay.

Four tests cover temporary/permanent/unexpected/malformed/truncated replies at
each transaction phase, exact sequencing and early stop, absent final reply,
write failure at every stage, QUIT failure after success, and a real verified-TLS
loopback session. The latter observes fresh EHLO, exactly one AUTH/envelope/DATA,
full prepared message bytes and dot transparency, then QUIT. All waits and observed
message buffers are bounded; boolean assertions avoid printing protocol inputs.
The generic transaction helper is private and used by scripted test streams only;
the submission entry point requires the opaque authenticated stream.

Next: separate bounded submission child framing and sanitized phase/acceptance
acknowledgments, with supervised slow/disconnected final DATA reply fixtures.
Durable frozen-message storage, retry integration, credential lookup, public review,
licensing, cluster validation and activation remain pending.

Validation: shared checks pass (181 Rust tests plus one ignored helper, 51 custom
process scenarios, eight SQLite tests, six hook tests), including formatting and
strict Clippy. Coverage passes the 80% gate at 96.15% Rust lines (9,361/9,736).

## Subsequent increment: supervised submission child boundary

A separate test-harness submission child consumes ten length-prefixed fields:
ephemeral fixture DER root, loopback port, username/password, single sender and
recipient, stable identity, subject, body, and frozen timestamp. Per-field ceilings
match the private builders (root 4,096; port 5; credentials 64/128; addresses
254 each; identity 64; subject 128; body 4,096; timestamp 10), totaling 9,139 bytes
including prefixes. Lengths are checked before allocation, exact EOF is required,
and credentials/message/trust are validated before connection. Empty body alone
is allowed. Owned field and writer buffers preserve zeroization; fixture trust is
explicit and certificate/hostname verification remains enabled.

The new SMTP_SUBMIT results distinguish pre-authentication blocks/failures from
MAIL/RCPT/DATA/final DATA reports. Only thirty canonical combinations can be encoded
or decoded; authentication success, delivery evidence at earlier phases, inconsistent
final acceptance, cross-mode tokens, raw text and trailing bytes are rejected.
All output fits 48 bytes. Final explicit refusal retains not-accepted evidence;
final reply I/O/protocol failures retain unknown acceptance. Parent timeout/failed
child/wrong token remains a transport error, conservatively unknown under the
existing parent policy; worker integration follows separately.

Three unit tests cover maximum-size input, every truncation, every oversized/empty
field, structured injection, date/port/UTF-8 rejection, and all canonical result
roundtrips/invalid combinations. Fifteen new custom process scenarios observe real
verified TLS/AUTH and exact single envelope/DATA bytes: acceptance, temporary and
permanent final refusal, malformed/unexpected final reply, abrupt EOF, stalled and
drip-fed final reply, early MAIL/RCPT refusal, invalid body/trust/trailing input
with no connection, wrong acknowledgment and valid acknowledgment from failed exit.
Stall/drip cases enforce a 500 ms request deadline, observe socket closure, and
return with cleanup within 1.5 seconds; drip confirms continuing traffic. Finite
socket waits remain independent of the parent deadline.

No dependencies, schema, runtime defaults or public interfaces changed.
Next: parent submission dispatch policy and owned outbox retry/rejection/acceptance
restart tests, including uncertainty when a child result is lost. Durable frozen
message storage, credential lookup, licensing, public review, cluster validation,
and activation remain pending.

Validation: shared checks pass (184 Rust tests plus one ignored helper, 66 custom
process scenarios, eight SQLite tests, six hook tests), including formatting and
strict Clippy. The 80% coverage gate passes at 96.21% Rust lines (9,625/10,004).

## Subsequent increment: durable submission result handling

The private parent submission classifier validates canonical child evidence again
for injected callers, preserves phase/diagnostic and explicit not-accepted evidence,
and maps valid final acceptance/refusal/retry outcomes into the existing worker.
Pre-authentication results reuse AUTH block/retry policy. Missing or failed child
results retain conservative unknown acceptance; forged combinations cannot invent
success. Reports distinguish relay acceptance from final refusal, pre-DATA failure,
and uncertain final results. This evidence is returned in memory; no new persistence
schema or diagnostic logging was introduced.

Two classifier tests verify decoded evidence and blocked/untrusted result handling.
A durable retry-budget test checks explicit temporary refusal and lost child result
across reopen through three attempts, preserving identity and completion-time retry
deadlines and preventing further dispatch after exhaustion. Two custom owned-store
scenarios compose real submission children: delivered, permanently rejected and
retrying rows commit before a preflight block, with a later destination untouched;
reopen preserves exact counts/identity/attempts/deadline and ownership. A relay-accepted
session discards its parent result and exits unsuccessfully after observing QUIT;
the parent schedules a retry, reopening preserves attempt two, and a second accepted
session delivers. Both servers verify the same frozen synthetic message bytes,
demonstrating possible duplicate submission rather than claiming deduplication.

The fixture identity/date/body are explicit test constants, not a durable message
storage contract or a rendering of the outbox event. Durable frozen-message mapping
remains separate work. Existing submission fixture checks were factored into a shared
bounded helper; strict TLS, stdin ownership, reply limits, and deadlines remain.
No dependencies, public interfaces, schema, or runtime defaults changed.
Next: private frozen message persistence design/experiment so retries and restart
use identical validated envelope/message bytes rather than reconstructing fixtures.
Credential lookup, public review, licensing, cluster validation and activation remain
pending.

Validation: shared checks pass (187 Rust tests plus one ignored helper, 68 custom
process scenarios, eight SQLite tests, six hook tests), including formatting and
strict Clippy. Coverage passes the 80% gate at 96.23% Rust lines (9,854/10,240).

## Subsequent increment: private frozen SMTP persistence

ADR 0008 records a private additive version-2 table keyed to the per-destination
outbox. Only a validated Prepared message creates immutable envelope/DATA snapshots;
identical writes are idempotent and conflicting bytes/envelopes fail. New snapshots
require an unattempted pending row and commit before any later network operation.
Existing version-1 rows upgrade without fabricated messages; future-version guard
now rejects version 3. Each migration transaction is atomic; 0001 remains unchanged.

Frozen loads validate byte bounds inside SQL before materialization, address policy
and canonical DATA transparency without rendering or sampling a clock. Snapshot
values have no Debug/Clone and own zeroizing DATA buffers. Missing and malformed
rows remain distinct. Outbox deletion cascades snapshots under existing retention;
pending protection remains with existing outbox policy. No credentials or relay
configuration are stored. Database encryption and authenticated corruption detection
are not introduced; this private loader is not a general MIME/header parser.

Four tests cover exact frozen bytes/envelope through retry/reopen, idempotent and
conflicting preparation, missing/attempted legacy rows, version upgrade/failure,
insert rollback, cascade, corrupt framing/address data and oversized BLOBs even
when schema checks are bypassed. No dependencies, runtime defaults or public
contracts changed; normal executable remains inactive. Private schema advances to
version 2 without a released downgrade guarantee.
Next: connect frozen reads to a separate bounded submission child frame, with
missing/corrupt snapshots blocking before network access and exact bytes reused
across retry/restart. Event rendering, atomic poll preparation, route revision,
credential lookup, public review, licensing, cluster validation and activation remain
pending.

Validation: shared checks pass (191 Rust tests plus one ignored helper, 68 custom
process scenarios, eight standalone version-1 SQLite tests, six hook tests), including
formatting and strict Clippy. Coverage passes the 80% gate at 96.50% Rust lines
(11,297/11,707).


## Subsequent increment: private frozen SMTP child submission

A separate bounded seven-field child frame transports saved envelope/DATA with
fixture trust/port and ephemeral credentials. Its 21,213-byte cap, per-field limits,
strict EOF, zeroizing buffers and canonical DATA/address checks run before network
access. The original renderer fixture stays separate; frozen restoration does not
render, sample a clock, add metadata or restuff DATA. Existing verified TLS/AUTH,
phase results, whole-process deadline and sanitized acknowledgments are reused.

A private load preflight blocks missing/corrupt/unreadable snapshots before child
construction. Owned-store acceptance proves no attempts/completion clocks on local
blocks and exact saved envelope/DATA through temporary refusal and accepted-send
lost-result retries across reopen. Conflicting new message preparation fails;
server fixtures observe the persisted bytes, including embedded metadata. Two
malformed frozen frame controls prove no TCP connection. Truncation, field bounds,
invalid port/credentials/envelope/DATA and trailing bytes have unit coverage.
Snapshot selection is explicit in this private harness; production batch wiring is
not activated. No dependencies, schema, defaults or public contracts changed.

Next: design/implement atomic private event, outbox and frozen-message preparation,
so a committed eligible SMTP event cannot lack its initial snapshot. Event rendering,
route revision, credential lookup, public review, licensing, real relay/cluster
validation and activation remain pending.

Validation: shared checks pass (193 Rust tests plus one ignored helper, custom
process acceptance, eight standalone version-1 SQLite tests and six hook tests),
including formatting and strict Clippy. Coverage passes the 80% gate at 96.54%
Rust lines (11,510/11,922). An overlapping check run encountered an existing owned
state lock reacquisition failure; the sequential shared-check rerun passed.


## Subsequent increment: patch private dependency advisories

Prioritize the push-reported security alerts before atomic SMTP preparation.
ADR 0009 records a complete lockfile OSV scan, verified advisory fixes and limits
on inspecting GitHub's authenticated alert state. Update private lettre/url/time
pins to 0.11.22/2.5.4/0.3.47; url now resolves only patched idna 1.1.0. A second
OSV scan reports zero affected versions across 79 locked registry packages.

Pin development and all CI compiler installs to Rust 1.88.0, required by patched
time; product MSRV/support remains undecided. Cargo lock format becomes 4.
Lettre stays builder-only and replaces chumsky with nom 8; obsolete stacker/psm
constraints and unneeded dependency entries are removed. New preflight cases
reject both Punycode hostnames from the IDNA advisory without connecting.
Existing transport, privacy, schema and runtime behavior remain unchanged.

Next: atomic private event/outbox/frozen-message preparation. Public review,
licensing, credential lookup, real relay/cluster validation and activation remain
pending.

Validation: Rust 1.88 shared checks pass (193 Rust tests plus one ignored helper,
custom process acceptance, eight SQLite and six hook tests), with strict Clippy,
formatting, tracked-file guardrails and rustdoc. Coverage startup now clears prior
raw profiles using cargo-llvm-cov clean --workspace --profraw-only plus crate
fallback cleanup; earlier profiles used an incompatible LLVM format. A fresh run
passes the unchanged 80% gate at 97.02% (8,497/8,758 Rust lines). Compiler/report
counting changes make this denominator incomparable to earlier accumulated runs;
no source exclusions were added. No GitHub alerts were suppressed or dismissed.


## Subsequent increment: atomic private SMTP poll preparation

ADR 0010 introduces a separate prepared storage batch with explicit SMTP route
subset and borrowed validated per-event/destination messages. New SMTP outbox rows
and their initial envelope/DATA snapshots commit with jobs, lifecycle ledger and
checkpoint in one BEGIN IMMEDIATE transaction. Rendering happens before storage;
no network/preparation callbacks execute under the transaction. Missing or unused
preparations abort, as do invalid/duplicate routes and event/preparation keys.
Existing private plain persistence preserves its earlier SQL failure behavior.

Four tests cover mixed plain/two-SMTP routes across two events, per-route recipient
and exact-byte preservation, retry/reopen and historical route/message immutability,
missing/extra/duplicate/unrequested preparations, incomplete second-route messages,
late snapshot/checkpoint failures with prior pending job and checkpoint rollback,
restart retry, legacy missing-snapshot preservation and checkpoint regression.
The owned TLS fixture now uses atomic preparation before temporary refusal and
accepted-send/lost-result replay across reopen. Schema remains version 2; no
new dependencies, public contracts or runtime defaults are introduced.

Next: compose this prepared storage path with private durable scheduler polling,
using a fallible bounded fixture preparation callback and publishing the candidate
in memory only after commit. Event rendering/redaction, route revisions, credential
lookup, public review, licensing, real relay/cluster validation and activation
remain pending.

Validation: shared checks pass (197 Rust tests plus one ignored helper, custom
process acceptance, eight standalone SQLite tests and six hook tests), including
formatting and strict Clippy. Fresh Rust line coverage passes the 80% gate at
97.17% (8,903/9,162). No real-cluster or real-relay tests were run.


## Subsequent increment: private prepared durable scheduler polling

A separate smtp_poll module shares the existing scheduler query/parser and atomic
prepared storage. It validates SMTP route selection and explicit message-count
limits, clones the reconciler, reconciles bounded query output, checks cardinality
before invoking any preparation callback, builds local messages and commits all
poll state before publishing the candidate in memory. No callback runs under the
SQLite transaction. Preparation failure is static and excludes raw payloads.

Four tests cover mixed routes/recipients/DATA, restored replay with changed routes
without rebuilding, preparation failure and count exhaustion without writes,
late storage failure preserving a previous pending job and checkpoint in memory
and SQLite, retry success, invalid settings, scheduler/parser errors and suppressed
discovery. Owned SMTP TLS acceptance now uses this composed polling path before
temporary-refusal and accepted-send/lost-result retry/reopen delivery fixtures.
The original plain durable poll, cycle error categories, schema and defaults stay
unchanged; no dependencies or public contracts are added.

Preparation callbacks are synchronous local fixtures: count and DATA size are
bounded, but an arbitrary blocked callback cannot be interrupted. Existing query
deadlines do not promise an end-to-end renderer deadline. Caller restores durable
state first; production routing/rendering remains pending.

Next: integrate prepared polling with the private bounded cycle, health reporting
and orchestration. Event rendering/redaction, route revisions, credential lookup,
public review, licensing, real relay/cluster validation and activation remain
pending.

Validation: shared checks pass (201 Rust tests plus one ignored helper, custom
process acceptance, eight standalone SQLite tests and six hook tests), including
formatting and strict Clippy. Fresh Rust line coverage passes the unchanged 80%
gate at 97.17% (9,217/9,485). No real-cluster or real-relay tests were run.


## Subsequent increment: private prepared cycle and health integration

Prepared SMTP cycle steps now share durable restoration, accounting-window
planning, finite budget consumption and health bookkeeping with plain cycles.
Separate fixture hooks select the SMTP subset and explicit maximum messages;
cycle destinations stay authoritative. No new production routing/defaults are
introduced. Static Preparation and PreparationLimit health categories distinguish
local message failure from exhausted message capacity. Existing invalid request,
query, storage and window failure mappings remain intact.

Three tests verify checkpoint age/staleness after preparation failure, reopened
success and preserved earlier frozen bytes, callback-free message-limit rejection,
cancelled/exhausted steps, consumed failure budget, invalid route/query/storage
categories, rollback and failure-streak reset. Owned SMTP child acceptance now
begins with a prepared cycle before atomic storage and frozen refusal/lost-result
retry/reopen. Shared step helpers preserve existing plain cycle behavior.

No schema, dependencies, public contracts or runtime defaults changed. Preparation
is synchronous and count/message-bounded; existing pre-cycle cancellation does not
interrupt in-flight queries or an arbitrary blocked callback.

Next: integrate prepared cycles into private delivery/retention/health orchestration
with its existing cancellation and partial-progress policy. Event rendering/redaction,
route revisions, credentials, public review, licensing, real relay/cluster validation
and activation remain pending.

Validation: shared checks pass (204 Rust tests plus one ignored helper, custom
process acceptance, eight standalone SQLite tests and six hook tests), including
formatting and strict Clippy. Fresh Rust line coverage passes the unchanged 80%
gate at 97.12% (9,448/9,728). No real-cluster or real-relay tests were run.

### Private prepared SMTP orchestration

Prepared polling now selects the shared fallible tick orchestration. SMTP route
and message-limit policy is checked before clocks, queries, delivery or cleanup,
including exhausted polling. Existing capacity/storage/window preflight,
completion clocks, cancellation, partial-progress failures, cleanup and health
reporting use the same implementation as plain polling. Preparation failures allow
previously committed pending work to continue; storage/window failures stop
maintenance. Cancellation after commit leaves frozen pending messages intact.

Three new tests cover cancellation after prepared commit, frozen state on reopen,
exhausted-poll delivery/health, invalid-route rejection before side effects and
pending delivery after preparation failure with an unchanged checkpoint and
sanitized health failure. Existing orchestration failure tests exercise the shared
engine. No schema, dependencies, public interfaces or runtime defaults changed;
synchronous preparation remains bounded by count/size but is not interruptible.

Next: owned SMTP child acceptance through the complete prepared tick, including
retry/reopen and blocked-dispatch partial progress. Rendering/redaction, route
revision, credentials, licensing, public review and real-cluster activation remain
pending.

Validation: shared checks pass with 207 Rust tests plus one ignored helper,
custom process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.11% (9,632/9,919), above the unchanged 80% gate. No real-cluster or
real-relay tests were run.

### Owned prepared SMTP tick acceptance

The owned frozen-submission fixtures now run the complete private prepared tick,
including atomic polling, delivery, retention and health. Temporary final refusal
and relay acceptance with a lost child result both produce a completion-based retry
at 25 after completion at 20. Reopening the owned store preserves the exact frozen
DATA and envelope, rejects changed content and suppresses duplicate preparation;
the retry succeeds through the verified loopback TLS child boundary.

A new three-destination prepared tick delivers the first route, then stops on a
missing helper for the second route. It preserves the first committed attempt,
reports a static spawn diagnostic without acceptance uncertainty and skips cleanup
and health. Reopen recovers the two pending routes with their original snapshots
and first attempt numbers, without resending the completed route. Final SQL state
contains exactly three delivery attempts.

Fixture-only secondary SQLite connections read committed snapshots while the
parent retains its owned-store guard. They close before child network calls. This
is acceptance plumbing, not a production storage/dispatch interface. Schema,
dependencies, runtime defaults and public contracts remain unchanged. Existing
missing/corrupt-snapshot rejection fixtures are retained.

Next: private SMTP event rendering/redaction design and bounded synthetic message
fixtures. Route revision, credential integration, licensing, public review and
real-cluster/relay activation remain pending.

Validation: shared checks pass (207 Rust tests plus one ignored helper, expanded
custom process acceptance, eight SQLite tests and six hook tests), including fmt
and strict Clippy. Fresh Rust line coverage is 97.11% (9,632/9,919), above the
unchanged 80% gate. Tests use synthetic scheduler replies and ephemeral verified
loopback TLS; no real cluster, relay or credentials were used.

### Private bounded SMTP lifecycle rendering

ADR 0011 records a test-only allowlist renderer with explicit Hidden/Identifier
job disclosure, fixed labels for emitted lifecycle states and strict 32-byte
numeric/array ID validation. Cluster and attempt context are always omitted;
unknown states, unsafe disclosed IDs and invalid message metadata fail with static
categories. Timestamp/envelope/identity are explicit caller inputs and use the
existing builder; no runtime defaults or production email format are activated.

Four renderer tests cover all emitted states, both disclosure choices, hostile
hidden fields, deterministic output, ID boundary/injection/Unicode failures and
metadata errors. A prepared-cycle rollback/reopen/recovery test verifies failed
rendering leaves no durable poll state and successful redacted bytes survive
reopen. Prepared cycle fixtures and the owned TLS partial-progress acceptance
now use this renderer before atomic commit, preserving duplicate suppression.

No dependencies, schema or public contracts changed. This minimal policy omits
unapproved free text rather than attempting general secret detection; production
identity/domain, credentials, route revisions, public review and activation remain
pending.

Next: mixed SMTP-route disclosure selection and later-route renderer-failure
rollback/restart acceptance. Licensing and real-cluster/relay validation remain
explicit unresolved gates.

Validation: shared checks pass with 212 Rust tests plus one ignored helper,
custom process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage passes the unchanged 80% gate at 97.23% (9,882/10,164). No real-cluster
or real-relay tests were run.

### Mixed-route disclosure and rendering-failure acceptance

A mixed-route poll fixture starts with an existing committed event and two frozen
SMTP snapshots. A subsequent two-event poll renders three messages successfully,
then fails the fourth with invalid explicit metadata. Candidate reconciliation is
not published and no new jobs, ledger, outbox, snapshots or checkpoint are written.
Reopen verifies the earlier ledger/checkpoint, two snapshots and three pending
routes remain intact. Recovery commits both new events, producing nine mixed-route
outbox rows; plain routes have no SMTP snapshot, route a hides job IDs and route b
includes validated numeric/array IDs. Exact per-route bytes and recipient addresses
are checked, earlier frozen content remains unchanged, and replay never renders.

Owned prepared-tick acceptance fails a later SMTP route after the first has
rendered, then reopens empty durable poll state. Recovery renders one hidden and
one disclosed snapshot, delivers both through verified loopback TLS children and
completes a synthetic plain provider route. It verifies first attempt numbers,
health recovery and exactly three durable attempts. Another reopen/poll suppresses
both preparation and delivery replay and preserves two frozen messages. Rendering
failure occurs before the SQLite transaction; existing transactional rollback
fixtures remain responsible for failures during SQL writes.

This increment changes tests only: schema, dependencies, production behavior,
privacy defaults and public interfaces remain unchanged.

Next: private per-route rendering policy validation before scheduler polling, with
explicit disclosure and bounded route metadata. Production identity/domain,
credential integration, route revisions, public review, licensing and real-cluster
or relay activation remain pending.

Validation: shared checks pass with 213 Rust tests plus one ignored helper,
expanded custom process acceptance, eight SQLite tests and six hook tests. Fresh
Rust line coverage is 97.23% (10,012/10,297), above the unchanged 80% gate.
No real-cluster or real-relay tests were run.

### Private validated SMTP rendering plans

ADR 0012 adds an immutable borrowed rendering plan with explicit per-route metadata
and disclosure. Collection/name bounds precede allocation and parsing; duplicate
or missing bindings, reused route Message-ID tokens and invalid count/envelope/ID/
date metadata fail during construction. A bounded local builder probe shares
actual message validation without scheduler, clock, storage or provider work.

The plan composes existing prepared polling with its fixed route bindings. Four
tests cover invalid metadata/bindings/limits, valid maximum name/count boundaries,
disclosure and unknown-route rejection, event-dependent oversized ID rollback,
reopen recovery and replay preserving earlier frozen disclosure. Non-SMTP routes
remain part of the atomic outbox without receiving snapshots.

No schema, dependencies, public contracts or production defaults changed. This
separate polling composition does not yet replace arbitrary prepared-tick hooks.

Next: validated-plan selection in private orchestration, preserving cancellation,
preflight, budgets and maintenance after event-dependent rendering failures.
Production per-event identity/domain, timestamps, credentials, route revisions,
licensing, public review and real-cluster/relay activation remain pending.

Validation: shared checks pass with 217 Rust tests plus one ignored helper,
custom process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.29% (10,265/10,551), above the unchanged 80% gate. No real-cluster
or real-relay tests were run.

### Validated rendering plans in private orchestration

A separate tick_rendered_fallible entrypoint selects immutable rendering plans
from explicit route inputs and the cycle's authoritative destination list. The
shared tick engine now accepts a deferred validation context: cancellation runs
first, general policy and plan validation precede clock/storage/query work, and
the validated plan feeds the existing prepared poll and health/error mapping.
Plain and arbitrary-preparation fixture entrypoints retain their shared behavior.
Invalid plan inputs report a static Policy preflight failure without consuming a
poll budget, including when polling is exhausted. Event-dependent rendering
failures retain the existing preparation-failure health and maintenance policy.

Two tests verify cancellation before invalid metadata validation, invalid policy
before side effects even when exhausted, frozen commit followed by cancellation,
reopen with changed disclosure and an oversized new event, continued delivery of
the earlier hidden snapshot, unchanged durable checkpoint/ledger and exhausted
maintenance without duplicate sends. The owned TLS three-route blocked-helper
fixture now runs validated rendering ticks, preserving first-route completion,
static spawn diagnostics, partial progress and original snapshots on recovery.

No schema, dependencies, public interfaces or executable defaults changed.
Validation probes and rendering are synchronous and bounded; cancellation does
not interrupt in-flight callback work. Static caller-selected fixture metadata
remains experimental.

Next: private per-event Message-ID and frozen timestamp selection design and
implementation, replacing reused fixture metadata before production routing.
Credential integration, route revisions, privacy defaults, public review,
licensing and real-cluster/relay activation remain pending.

Validation: shared checks pass with 219 Rust tests plus one ignored helper,
custom process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.31% (10,444/10,733), above the unchanged 80% gate. No real-cluster
or real-relay tests were run.

### Private per-event SMTP metadata selection

ADR 0013 replaces literal validated-route message IDs/dates with explicit route
namespaces and bounded SHA-256 event/state/destination/namespace selection. Date
comes from the poll's observed time; identity excludes time so retries remain
stable. Empty actual-start context is allowed for pre-execution cancellation.
Selected IDs contain no raw identity fields, but are fingerprints rather than
anonymity guarantees. Existing atomic preparation freezes the exact resulting
headers and replay skips rendering. The Message-ID domain remains fixture-only.

Reuse already-locked ring 0.17.14 as a direct private dev dependency for incremental
SHA-256; only the lockfile root dependency edge changes. No registry versions,
schema, public interfaces or executable defaults change. The strict builder probe
validates explicit namespaces and envelopes; shared namespaces are now valid
because destination names are hashed too. Route-level timestamps are removed from
this private plan; the low-level arbitrary fixture renderer remains explicit.

Tests cover a golden vector, event/state/route/namespace distinctions, field
boundaries, stable IDs under changed observation time, cancellation without start
context and work/date bounds. Actual mixed-route headers prove four distinct IDs
for two events/two SMTP routes and exact observation Dates. Reopen at a later time
preserves frozen bytes and outbox identities. Existing validated orchestration and
owned blocked-dispatch TLS acceptance use selected metadata.

Next: complete owned TLS retry/lost-result acceptance using selected per-event
metadata. Production domains, credentials, route revisions, public review,
privacy defaults, licensing and real-cluster/relay activation remain pending.

Validation: shared checks pass with 223 Rust tests plus one ignored helper,
custom process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.38% (10,657/10,944), above the unchanged 80% gate. No real-cluster
or real-relay tests were run.

### Selected metadata through owned SMTP retry/restart

The owned frozen submission fixture now begins with a validated rendering tick
and selected per-event identity/observed Date for both temporary final refusal and
relay acceptance with a lost child result. Initial preparation occurs at 10,
completion at 20 and retry becomes due at 25. Reopening preserves exact DATA and
envelope; assertions check the selected hexadecimal Message-ID and Date at 10.

Recovery deliberately supplies a changed namespace, sender, recipient and job-ID
disclosure policy. A freshly prepared candidate would differ, and replacing the
existing snapshot fails. Duplicate reconciliation emits zero new events. The
retry loads and sends the original snapshot through the verified loopback TLS
child, completes at 30 and leaves no pending delivery. Final SQL attempt history
is exactly transient_failure at 20 then success at 30. Server-side fixtures compare
full DATA/envelope to the snapshot, including for the lost-result path; relay
acceptance still permits at-least-once resend when the parent loses the result.

Existing missing/corrupt-snapshot local rejection coverage is retained. This
increment changes acceptance fixtures only; no schema, dependencies, public
contracts or executable defaults change.

Next: private durable route-revision binding design, preserving prepared snapshots
and defining how changed routing applies to newly discovered events. Production
Message-ID domains, credentials, privacy defaults, public review, licensing and
real-cluster/relay activation remain pending.

Validation: shared checks pass with 223 Rust tests plus one ignored helper,
expanded custom process acceptance, eight SQLite tests and six hook tests. Fresh
Rust line coverage is 97.35% (10,654/10,944), above the unchanged 80% gate.
No real-cluster or real-relay tests were run.

### Private durable SMTP route-revision design

ADR 0014 defines immutable rendering revisions bound to new SMTP outbox rows in
the prepared poll transaction. Historical snapshots/ledger outcomes remain fixed;
new policy applies to newly emitted events. Legacy rows retain their contents
without inferred provenance and block the future revision-aware dispatch path.
Explicit permitted revisions govern pending delivery; transport cannot silently
fall back to current configuration. Bounded cleanup preserves pending references.

The design separates a rendering descriptor from the required later provider
binding for relay/TLS/auth identity and credential references. Existing fixtures
freeze envelopes/DATA but still select transport through caller hooks, so this
milestone does not claim durable production provider routing. Secrets never enter
revision descriptors. Public configuration/defaults remain unapproved.

Next: implement only the pure bounded canonical rendering descriptor and SHA-256
fingerprint from validated plans, with policy-change/order/boundary tests. Later
small milestones add an additive private schema, atomic bindings, versioned message
identity, controlled dispatch/cleanup and durable provider profiles.

This milestone changes documentation only. Schema stays 2; dependencies,
executable behavior and the 80% coverage gate are unchanged. Production domains,
credentials, privacy defaults, licensing, public review and real-cluster/relay
activation remain pending.

### Pure rendering descriptor implementation

The first private increment is implemented in smtp_revision. Descriptor::from_plan
accepts only an immutable validated Plan and an SMTP route lookup. It records
ordered destination, sender, recipient, namespace, disclosure, renderer version,
identity-selection version and Message-ID domain fields. Encoding begins with the
private version marker, then unsigned big-endian u16 tag lengths and u32 value
lengths followed by UTF-8 bytes. The complete 4,096-byte bound is checked before
allocation, copying or hashing. Canonical bytes use a zeroizing buffer; the
container omits Debug/Clone and exposes only borrowed bytes/token. Errors contain
static Route/Limit categories.

Existing renderer/identity versions are explicit private constants. The message
builder's fixture domain is centralized without changing generated messages.
Fingerprints use the already locked ring SHA-256 implementation; dependencies
remain unchanged. Tokens are exactly 64 lowercase hexadecimal bytes.

Five tests verify independent golden encoding/digest, changes to all route policy
fields and encoded version/domain inputs, equivalent route ordering and operational
limits, unknown/plain-route rejection, maximum validated address/name/namespace
inputs, invalid policy rejection and exact/over-limit encoding boundaries. Typed
lengths distinguish ambiguous tag/value concatenations. Descriptors exclude event
identity, observation time, credentials and operational limits.

Schema stays 2. No revision records, outbox bindings, versioned revision-aware
Message-IDs, provider selection, public contracts or runtime defaults are activated.
Next: additive private migration and immutable descriptor/binding storage, with
bounded canonical validation, legacy preservation, conflicts and rollback tests.
Atomic poll integration and durable provider profiles remain later increments.

Validation: shared checks pass with 228 Rust tests plus one ignored helper,
custom process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.38% (10,836/11,127), above the unchanged 80% gate. No real-cluster
or real-relay tests were run.

### Private revision storage implementation

Additive migration 0003 advances private SQLite state to version 3. Immutable
rendering descriptors are keyed by cluster, destination and fingerprint; bindings
reference both the outbox/snapshot and descriptor. Insert-time cluster checks and
foreign keys reject incoherent references. Outbox removal cascades snapshots and
bindings; unreferenced descriptors remain for later bounded cleanup.

Explicit freeze_smtp_revision preparation atomically inserts the descriptor, new
snapshot and binding, or compares all existing values for idempotence. Conflicts
and late binding failures roll back every write. Existing unbound snapshots are
never assigned inferred provenance. Legacy polling/delivery remains unchanged.

Stored descriptors are bounded in SQL before materialization, decoded in fixed
field order, revalidated through Plan, and checked against the canonical SHA-256
fingerprint. Unknown versions, malformed fields, missing references and envelope
mismatches fail closed with static errors. Migration, reopen, immutable conflicts,
legacy preservation, malformed state, foreign keys and rollback have fixture tests.

Next: integrate these records into atomic prepared polling and versioned event
identity selection. Revision-aware dispatch, explicit revision permission and
durable relay/TLS/auth profiles remain subsequent work. No public contracts,
new dependencies, runtime defaults, real-cluster tests or releases are introduced.

Validation: shared checks pass with 233 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.46% (11,230/11,523), exceeding the unchanged 80% gate.

### Atomic revision-bound polling and identity selection

Plan::poll_revision_with is an explicit private path that derives one immutable
v2-identity rendering descriptor per validated SMTP route before querying. It
prepares bounded messages with revision-aware Message-IDs, then commits jobs,
event ledger, outbox, snapshots, descriptors, bindings and checkpoint in the same
transaction. Candidate memory is published only after commit. Descriptor conflicts
and late binding/checkpoint failures roll back all new state. Plain destinations
receive no SMTP descriptor or binding. Revisions are inserted only for newly
prepared pairs; empty/suppressed/replayed polls leave no orphan descriptors.

The v2 identity hashes the existing length-prefixed event/route/namespace inputs
plus the exact rendering revision token under a new domain separator. Observation
time remains outside the ID and supplies the frozen Date. Descriptors explicitly
record sha256-event-route-revision-v2. Stored decoding accepts both known private
identity versions; legacy preparation and existing frozen bytes remain unchanged.
No schema migration, dependency or public contract change is needed.

Tests verify changed policy for new events, original bound bytes after reopen,
legacy and suppressed history without backfill, new destination fan-out, canonical
token/bytes conflicts, late binding/checkpoint rollback, recovery, rendering/count
failures and invalid revision preflight before scheduler work. An independently
computed v2 digest anchors identity tests; Date changes do not change identity.

Next: revision-aware controlled dispatch with explicit permitted revisions and
bounded unreferenced cleanup. Durable relay/TLS/auth profile binding remains
required before production routing. Runtime orchestration still uses the existing
legacy fixture path; no operational executable or real-cluster activation occurs.

Validation: shared checks pass with 239 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.45% (11,797/12,106), above the unchanged 80% gate.

### Revision-aware controlled delivery and bounded cleanup

The private smtp_revision_delivery worker requires a validated immutable allowlist
of (cluster, destination, rendering revision) entries. At most 64 entries are
accepted; an empty list denies all. Each due row loads and validates its durable
descriptor/snapshot before checking permission. Missing legacy bindings, malformed
state and denied revisions block with static diagnostics before sending, sampling
a completion clock or retiring exhausted retries. Approved sends receive the bound
descriptor and original snapshot, without consulting current rendering policy.

The shared controlled worker now accepts a checked authorization context before
budget retirement. Existing callers retain their prior behavior through a trivial
authorization adapter. Cancellation, partial committed progress, completion-based
retry scheduling and uncertain acceptance bookkeeping keep their existing rules.
Tests cover permission bounds, cluster/destination isolation, legacy/malformed
blocks, denied exhausted rows, partial progress, cancellation between sends, exact
retry bytes after reopen and completed snapshot/binding retention cleanup.

Store::prune_smtp_revisions removes at most 256 unreferenced descriptors for one
explicit cluster in an immediate transaction. Every binding protects its revision,
including pending and completed rows until ordinary outbox retention removes them.
A late-delete failure rolls back the complete cleanup batch. Tests verify bounds,
cluster isolation, late rollback, reopen and retained pending references. Jobs,
event history and checkpoints remain intact.

Owned loopback TLS acceptance exercises both temporary submission refusal and lost
child-result retry through this guard. Reopen with an empty allowlist blocks before
child work without another attempt; restoring the explicit permission delivers the
exact original snapshot and retains the two-attempt durable history. Relay/trust/
authentication profiles are still caller-selected fixture inputs: this guard is
rendering permission only, not durable production provider routing.

Next: design and implement durable provider-profile descriptors and bindings, then
verify exact relay/TLS/auth identity across restart. Public activation, credential
integration, privacy defaults, licensing and real-cluster/relay validation remain
separate gates. No new dependencies, schema versions or executable defaults change.

Validation: shared checks pass with 246 Rust tests plus one ignored helper, custom
process acceptance including guarded owned TLS retry/reopen, eight SQLite tests
and six hook tests. Fresh Rust line coverage is 97.52% (12,288/12,601), above
the unchanged 80% gate. All network acceptance uses temporary loopback fixtures.

### Pure SMTP provider-profile descriptor

ADR 0015 specifies the next provider-binding increments. The first bounded step
implements private smtp_provider_revision descriptors covering destination, relay,
port, TLS mode/server name, trust reference/content hash, AUTH PLAIN identity and
credential reference. Canonical bytes are bounded, zeroizing and SHA-256 hashed;
stored decoding rejects malformed fields, unknown versions and token/bytes
mismatches. Profile resolution can compare exact validated content through matches.

A bounded DER trust-set helper validates certificate parsing, rejects duplicates,
and sorts before length-prefixed hashing so input order does not change identity.
Tests cover an independent golden digest, all policy fields, exact matching, DNS/IP
canonical spelling, unsafe inputs, limits, stored corruption and changed trust.
No secrets are stored in profiles, and no lookups or network calls occur here.

Next: additive provider revision/binding storage and atomic preparation. Then bind
resolved relay/TLS/auth transport and explicit provider-pair permissions to those
records, with owned endpoint/trust/identity restart acceptance. This increment
adds no dependencies, migrations, public contracts or executable activation.

Validation: shared checks pass with 252 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.58% (12,688/13,003), above the unchanged 80% gate. No real relay
or cluster was contacted.

### Private provider storage and atomic preparation

Additive migration 0004 advances the private schema to 4. Provider descriptors
are keyed by cluster/destination/token; bindings reference the outbox and the
rendering binding with the same cluster. Foreign keys protect referenced provider
records and cascade bindings during completed outbox cleanup. Unreferenced
provider descriptors remain for a later bounded cleanup operation.

Store::persist_prepared_profiles requires a complete, unique provider descriptor
set matching the validated rendering destinations. It commits jobs, ledger,
outbox, frozen messages, both descriptor sets, both bindings and checkpoint in
one transaction. Existing token/bytes conflicts fail without overwriting records.
Only newly prepared pairs insert descriptors/bindings; replay does not backfill
legacy or render-only snapshots. Changed profiles apply to newly emitted events
and preserve existing profiles and exact message bytes after reopen.

Bound provider loading limits canonical BLOBs in SQL before materialization,
validates the fingerprint/profile and checks the cluster, destination, rendering
binding and snapshot. Malformed or missing referenced records fail closed. Tests
cover schema upgrades and failed migration recovery, immutable conflicts, route
coverage, late binding/checkpoint rollback, reopen, changed policy, missing/corrupt
records, foreign-key coherence and completed cleanup while preserving the ledger.

Next: connect immutable provider plans to scheduler polling, then enforce explicit
rendering/provider-pair permission and exact resolved relay/TLS/auth identity before
transport. This increment is the private atomic storage boundary; existing polling
and delivery adapters retain their prior fixture behavior. No new dependencies,
public contracts, executable activation or real-cluster/relay testing is introduced.

Validation: shared checks pass with 258 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.66% (13,209/13,526), above the unchanged 80% gate.

### Immutable provider-aware polling

Private ProviderPlan captures one validated provider descriptor and v2 rendering
descriptor per SMTP route before scheduler work. Its constructor rejects missing,
duplicate, unknown and invalid profiles; profile order does not select routing.
No Debug/Clone implementation exposes profile metadata. Polling validates Date,
uses the immutable route policy to render bounded messages and invokes the atomic
profile-storage boundary. Reconciliation memory is published only after commit.

The shared private preparation worker retains legacy/render-only adapters and now
has explicit profile settings with route coverage preflight. Provider-aware polls
commit both binding sets with messages, jobs, ledger and checkpoint. Plain routes
receive no SMTP profiles. Changed profiles apply only to new events; restored
history preserves its original profile and bytes. Suppressed and render-only
history does not receive inferred provider provenance on replay.

Four tests cover invalid/incomplete/duplicate profiles, reordered valid profiles,
new-policy selection with reopened old snapshots, deduplicated replay, late binding
rollback with unchanged memory/checkpoint and reopen recovery, invalid Date before
queries, rendering failure and suppressed/render-only history without backfill.
Existing Message-ID algorithms and delivery adapters are unchanged.

Next: validate explicit rendering/provider-pair permissions and resolve the exact
bound relay, trust and authentication identity before transport, with owned TLS
endpoint/restart acceptance. No new schema, dependencies, public contracts, runtime
defaults, installed monitoring or real-cluster/relay activation is introduced.

Validation: shared checks pass with 262 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.64% (13,560/13,888), above the unchanged 80% gate.

### Provider-pair permission and validated resolution guard

Private smtp_provider_delivery requires an immutable allowlist of at most 64
(cluster, destination, rendering revision, provider revision) entries. Empty lists
deny all; duplicate pairs, invalid names and malformed tokens are rejected. Each
due row loads both durable descriptors and checks the complete pair before local
resource resolution. Missing/corrupt bindings and denied pairs block before sending,
clock sampling or exhausted retry retirement. Existing fixture adapters remain.

Opaque Resolved construction compares the supplied profile to the bound descriptor,
hashes and validates the actual DER trust set, requires the exact authentication
identity, and validates credentials using the existing zeroizing AUTH PLAIN type.
No credentials are persisted or exposed through diagnostics. The worker compares
the returned context's canonical bytes/token with the stored provider again; a
resolver cannot substitute a context validated against another profile. Approved
sends receive the original snapshot and consume the validated profile/resources.
Secret rotation under the same reference/identity keeps the provider token stable.

Five tests cover pair validation and all matching fields, actual trust/auth identity
checks, secret rotation, missing legacy/render-only/corrupt bindings, denial before
resolution or retry retirement, resolution errors, wrong returned contexts,
cancellation and exact snapshot/pair replay across reopen with completion-based
retry scheduling. Diagnostics contain static categories only.

This increment validates caller-injected local resolution and guards an injected
send callback. It does not yet build a concrete network transport from Resolved;
an arbitrary callback is not proof of bound endpoint use. Next: construct verified
TLS/authenticated submission from the validated context and verify exact endpoint,
trust and authentication identity in owned loopback restart/lost-result acceptance.
Provider cleanup and orchestration integration follow separately. No new schema,
dependencies, public contracts, installed credentials/monitoring, real-cluster tests
or production activation are introduced.

Validation: shared checks pass with 267 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.63% (14,031/14,371), above the unchanged 80% gate.

### Concrete supervised bound SMTP submission

Private smtp_bound_transport consumes the validated resolution context and frozen
message through a bounded zeroizing stdin frame. The child revalidates canonical
provider identity, actual DER trust fingerprint, AUTH identity, envelope and DATA
before connecting to the bound relay/port. Required STARTTLS verifies the bound
server name against only the supplied trust roots, then authenticates and submits
the original bytes. Static acknowledgments use the existing retry classification.
Implicit TLS is rejected before spawning; no fallback selects another transport.

The parent supervisor bounds DNS, input, TLS, socket work and child output with an
absolute deadline. Child socket deadlines accept whole milliseconds from 1 to
30,000 and connection attempts inspect at most 16 resolved addresses. The helper
command remains a private trusted fixture choice, not an installed CLI contract.

Three unit tests cover frame round trips, every truncated frame prefix, trailing
bytes, field bounds, invalid deadlines and unsupported TLS before spawn. Owned
loopback acceptance keeps the original relay alive across SQLite reopen, verifies
TLS name and AUTH identity, denies changed settings before a decoy connection,
rotates the secret under the same identity and replays exact DATA after temporary
failure or a lost child result. Accepted sockets explicitly use blocking I/O.

Next: bounded unreferenced provider revision cleanup, then private orchestration
integration. No schema, dependency, public contract or production activation changes.

Validation: shared checks pass with 270 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.46% (14,319/14,692), above the unchanged 80% gate.

### Bounded SMTP provider revision cleanup

Private Store::prune_smtp_provider_revisions validates a nonempty cluster of at
most 4096 bytes and a deletion limit of 1..=256. One immediate transaction selects
unreferenced provider descriptors in destination/revision order, rechecks binding
absence on deletion and commits the actual removal count. Every existing binding
protects its descriptor, including pending and completed work awaiting retention.
The operation leaves rendering descriptors, snapshots, ledger and attempts intact.

Tests cover invalid bounds, bounded batches, cluster isolation, a late trigger
failure rolling back earlier deletions, pending protection and exact frozen bytes
after reopen. Completed-retention acceptance verifies cascading binding removal
makes the provider eligible for pruning and that deletion survives reopen without
removing event history. No schema, dependency or public contract changes occur;
cleanup remains caller-invoked with no new runtime default.

Next: private orchestration integration of provider-aware polling, pair permission,
validated resolution and supervised bound dispatch. Production activation and public
CLI/config contracts still require their separate review and acceptance gates.

Validation: shared checks pass with 271 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.51% (14,436/14,804), above the unchanged 80% gate.

### Private bound SMTP orchestration

Cycles::tick_bound_smtp composes provider-aware atomic polling with rendering/provider
pair permissions, validated caller-owned resolution and concrete supervised bound
STARTTLS submission. The existing stage engine now accepts a private delivery
policy; legacy ticks retain their delivery adapter. Poll validation follows initial
cancellation and precedes clocks/storage/queries, including exhausted cycles.
Profiles must cover all destinations: this increment deliberately supports only
SMTP routes. Timeout preflight requires whole milliseconds within 1..=30,000.

Existing stage ordering preserves durable poll state before sending, cancellation
between stages, monotonic completion clocks, partial delivery failures, retention
and health reporting. A blocked dispatch stops maintenance without inventing a
successful attempt. Helper command selection and secret/trust resolution stay
caller-owned; no environment lookup, installed helper, scheduling or cleanup default
is added. Provider revision pruning remains an explicit caller operation.

A network-free process fixture tests cancellation before validation and after poll
commit, invalid deadline preflight, denied pairs after reopen, exhausted polling
with pending delivery, temporary retry and final acceptance with unchanged frozen
bytes. Its synthetic acknowledgments exercise bookkeeping only. Owned TLS
acceptance now runs temporary/lost-result submission and reopened secret-rotation
recovery through the orchestration method, verifying the original endpoint, TLS
identity, AUTH identity and exact DATA. Changed profiles still fail before a decoy
connection. No schema, dependency or public contract changes occur.

Next: mixed-route dispatch policy and acceptance, preserving bound SMTP checks
alongside ordinary destinations. Public contract review, real-cluster policy
validation, installed monitoring and release gates remain outstanding.

Validation: shared checks pass with 272 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.52% (14,676/15,049), above the unchanged 80% gate.

### Private mixed-route dispatch

Cycles::tick_mixed_bound_smtp validates an explicit partition of at most 64
SMTP/ordinary destinations before clocks, storage or queries. Duplicate, overlapping,
unknown or incomplete ordinary lists fail preflight. Ordinary names are bounded
ASCII identifiers. Provider-aware polling atomically prepares SMTP snapshots and
both descriptor bindings while ordinary destinations retain their existing outbox
format. The SMTP-only tick delegates with an empty ordinary list.

The mixed worker shares the existing pair authorization routine with SMTP-only
delivery. Ordinary rows use only the explicit ordinary callback and do not resolve
SMTP credentials. A bounded existence query checks all three durable SMTP artifact
tables first: any message, rendering binding or provider binding rejects an ordinary
send. This also protects legacy/render-only snapshots and malformed bound state
without loading message contents. Unlisted rows go through the SMTP guard and cannot
fall back after missing bindings, permission denial or resource-resolution failure.
Authorization still precedes exhausted-budget retirement. Static blocks stop the
batch and maintenance while reporting already committed progress.

Tests cover invalid route partitions, cancellation after atomic preparation,
ordinary success before a denied SMTP pair, retained partial progress after reopen,
SMTP retry/recovery without ordinary replay, and downgrade rejection for legacy,
render-only and provider-bound snapshots. Owned TLS acceptance now includes an
ordinary destination alongside temporary/lost-result SMTP submission and reopened
secret rotation, preserving exact DATA and the original endpoint. Ordinary transport
remains caller-injected; its existing provider tests validate transport separately.
No schema, dependency, public contract or runtime-default changes are introduced.

Next: mixed-route mid-batch cancellation and blocked-resolution acceptance, then
review the remaining prototype-to-product gaps. Public contracts, installed runtime,
real-cluster validation and release gates remain separate outstanding work.

Validation: shared checks pass with 274 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.45% (14,966/15,358), above the unchanged 80% gate.

### Mixed-route cancellation and blocked-resolution recovery

New process-fixture acceptance cancels after ordinary delivery has committed and
before pending SMTP resolution. Reopened state retains ordinary progress, exact
SMTP DATA, both revision tokens and attempt number one. Subsequent trust, profile,
credential and wrong-context failures block before helper creation or completion
clock sampling. Failure reports remain static and show no uncertain acceptance;
cleanup/health stages are skipped. A later approved resolution completes the pending
row without repeating ordinary delivery. Assertions check callback and clock counts.

Owned TLS restart acceptance now tests changed-profile resolution through the full
mixed tick rather than directly invoking the guard. It verifies no helper or ordinary
replay, no decoy connection, unchanged retry attempt and recovery through the original
bound relay. Existing temporary/lost-result acceptance remains intact. No production
code, dependencies, defaults, schema or public contracts change in this increment.

The README now describes the private prototype accurately. A current product-readiness
assessment separates verified fixture behavior from the missing installed runtime,
contract review, onboarding, platform/cluster validation and release work.

Next: prepare a concrete CLI/config/event contract review package using the existing
proposals, before activating an installed runtime. Continue private transport deadline
acceptance where needed; do not infer approval of public surfaces from prototype tests.

Validation: shared checks pass with 275 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests and six hook tests. Fresh Rust line
coverage is 97.47% (15,163/15,557), above the unchanged 80% gate.

### Concrete local contract review package

Prepared a proposed CLI grammar with side effects, exit policy, setup/test behavior,
foreground cancellation and output acceptance. Added an SMTP config extension for
explicit relay/TLS/trust/auth resources, frozen profiles, exact pair permissions and
pending-history preservation. The synthetic mixed-provider example uses environment
names and placeholder fingerprints; it is not runnable configuration or authorization.
Refreshed the webhook config draft to distinguish existing private capabilities from
installed/product gaps and linked all drafts through a decision/review checklist.

The package recommends strict JSON, explicit operational values, suppress as the
setup recommendation for initial observations, minimal opaque-ID payloads, and a
foreground webhook runtime slice followed by SMTP before the MVP is complete.
Recommendations remain unapproved. Resource upper bounds, support/timezone semantics,
historical-profile UX and compatibility fixtures still need review before schema
freeze. No dependencies, schemas, CLI behavior, defaults or approved baselines change.

Next: review the package and settle limits/semantics, then add approved snapshots and
schemas before runtime activation. AGENTS.md requires explicit review for public
CLI/config/event surfaces; a continuation request alone does not settle these new
proposed decisions. Further draft refinement and private gap fixes can proceed.

Validation: shared checks pass unchanged with 275 Rust tests plus one ignored
helper, custom process acceptance, eight SQLite tests and six hook tests. Draft
JSON syntax, duplicate keys and provider/permission references were checked. No Rust
changes; the last measured Rust line coverage remains 97.47% (15,163/15,557).

### Draft resource limits and boundary acceptance

Added concrete proposed parsing/count/byte/duration ceilings, cross-field checks,
referenced-resource bounds and post-review boundary acceptance. Explicit stale,
SMTP preparation and maintenance keys complete the operational draft; examples now
include them. A machine-readable range manifest supports review only, not schema
validation. Authorization remains within the private 4096-byte bound; SMTP limits
retain current private ceilings. Retry-After remains an independent floor.

All values remain proposed ceilings, not defaults or cluster recommendations.
Protected retention/history and rollback under preparation pressure remain explicit;
no total RSS/SQLite disk-size guarantee is invented. No production implementation,
dependency, public surface or approved contract baseline changes.

Next: explicit review of the complete contract package and limits, then approved
schemas/snapshots and the smallest foreground runtime activation. Private fixtures
and draft refinement may continue without public activation; AGENTS.md requires
explicit review before introducing the proposed public CLI/config/event contracts.

Validation: shared checks pass unchanged with 275 Rust tests plus one ignored
helper, custom process acceptance, eight SQLite tests and six hook tests. Both
configuration examples pass draft range, structural, cross-field and reference
consistency checks; all proposal JSON parses without duplicate keys. No Rust changes;
the last measured Rust line coverage remains 97.47% (15,163/15,557).

### Private draft-configuration fixture validation

Added a Python standard-library fixture-only validator and 11 tests to the shared
local/CI/staged/pre-push suite. Config reads are supplied bytes bounded to 65,536;
a lexical depth scan precedes JSON parsing, decoded duplicate keys reject, strict
integer tokens fit u64 and fractional/exponent/nonfinite values reject. Decoded
object/array counts, known fields, draft ranges, policies, references and cross-field
relationships are checked with static diagnostics. Unicode surrogate text errors
also become static categories; rejected input is not echoed through exception chains.

Tests exercise min/max/out-of-range numeric values, byte/depth/member/array budgets,
escaped duplicate keys, unknown/missing keys, schema versions, route/permission
references, SMTP trust path lists, invalid environment names and empty deny/suppress
configurations. Validating examples opens no referenced resource. The harness does
not implement file/env lookup, DER/URL/SMTP semantic validation or installed parsing.
No Rust dependency, public contract, schema or runtime default changes are made.

Next: extend private installed-loader preparation only within reviewed scope; the
complete CLI/config/event package still needs explicit review before public contract
baselines or runtime activation. The Python harness is review evidence, not approval.

Validation: shared checks pass with 275 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests, six hook tests and 11 draft configuration
tests. The additional count-ceiling fixtures pass. No Rust changes; the last
measured Rust line coverage remains 97.47% (15,163/15,557); Python fixture code is
explicitly outside the Rust coverage metric.

### Private owned configuration byte reader

Added test-only config_snapshot using existing locked libc/zeroize dependencies.
Descriptor-relative no-follow/nonblocking reads reject unsafe paths, ancestors,
parent/file modes, symlinks, hard links and special files. Metadata bounds and an
independent 65,537-byte read budget protect the 65,536-byte snapshot ceiling.
Opaque zeroizing bytes publish only after metadata/byte-count rechecks and remain
owned after source edits/removal. No file creation, lookup defaults or secret/path
content appears in diagnostics.

Five tests cover stream budget, interrupted/I/O reads, private-file reopen, owned
bytes after edits/removal, path/mode/link/special-file rejection, empty/oversized
files and observed read-time mutation. ADR 0016 records Unix scope, metadata race
limits and the absence of an absolute filesystem deadline. Python draft validation
remains separate; there is no installed cross-language configuration pipeline.

Next: private descriptor replacement/mutation acceptance and resource-loading
preparation, then a reviewed parser/config-to-plan mapping. Public CLI/config/event
contracts and installed activation still require explicit review. No new dependency,
schema, runtime default or supported product path is introduced.

Validation: shared checks pass with 280 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests, six hook tests and 11 draft configuration
tests. Rust line coverage is 97.49% (15,391/15,788), above the 80% gate; Python
fixture validation remains outside that metric.

### Private configuration replacement and mutation acceptance

Added three deterministic filesystem acceptance tests covering atomic replacement
of an opened file, symlink substitution after reading, same-size rewriting with an
explicit timestamp change and permission changes before publication. Rechecks
reject the old unlinked descriptor, observed metadata changes and unsafe modes.
Fresh reads accept the replacement regular file and reject substituted symlinks.
No implementation behavior, dependencies, public contract or runtime defaults change.

Next: private resource-loading preparation; installed parser/config-to-plan mapping
and activation still require explicit contract review. Metadata observation remains
subject to the writer/race limits in ADR 0016.

Validation: shared checks pass with 283 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests, six hook tests and 11 draft configuration
tests. Rust line coverage is 97.48% (15,452/15,851), above the 80% gate.

### Private owned trust resource loading

Added test-only trust_snapshot to load caller-selected DER files through protected
configuration-reader descriptors, with count/path preflight and a shared 65,536-byte
budget. Each read receives the remaining budget plus one sentinel for overflow;
owned roots and their existing SMTP content fingerprint publish only after all
reads and DER validation succeed. Duplicate paths/content and malformed or oversized
sets fail with static diagnostics. No ambient trust, dependencies or runtime defaults.

Three tests cover valid owned/reordered reopen, rotation/removal, duplicate/malformed
DER and count/path/aggregate ceilings. The reader's existing tests also exercise its
reduced and invalid limits. ADR 0017 records sequential-loading and filesystem limits.

Next: private credential/resource resolution preparation. Installed configuration
mapping and activation remain gated by explicit public contract review.

Validation: shared checks pass with 286 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests, six hook tests and 11 draft configuration
tests. Rust line coverage is 97.52% (15,589/15,986), above the 80% gate.

### Private injected SMTP credential resolution

Added test-only credential_resolution with name/identity preflight, requested
64/128-byte source ceilings, exact authentication identity matching and existing
AUTH validation. Owned zeroizing temporary values become owned zeroizing credentials
only on success. Username failures block password lookup; diagnostics stay static.
No ambient environment lookup, dependencies, defaults or installed contracts change.

Four tests cover exact ownership/rotation, accepted limits, rejected references and
identities without lookup, mismatch/source failure, invalid UTF-8, empty/oversized
passwords and controls. ADR 0018 records callback allocation/deadline limitations.

Next: private provider resolution combining trust and credentials with immutable
provider binding. Installed configuration mapping and activation still require
explicit public contract review.

Validation: shared checks pass with 290 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests, six hook tests and 11 draft configuration
tests. Rust line coverage is 97.47% (15,706/16,113), above the 80% gate.

### Private bound provider resource composition

Added test-only provider_resolution to reject changed/invalid immutable profiles
before file or secret lookup, load owned bounded trust and check its fingerprint
before credential lookup, then return the existing resolved provider with owned
credentials. The new owned-credential constructor independently rechecks binding,
authentication identity and DER fingerprint. No permission grant or send occurs.

Five tests cover successful ownership/password rotation, removal after resolution,
profile/reference mismatch gates, missing/changed trust, credential failure and
constructor rechecks. ADR 0019 records the sequential-resource/deadline limits.
No dependencies, runtime defaults or public contracts change.

Next: private composed resolution in permission-guarded pending delivery with
failure/restart preservation. Installed configuration mapping and activation still
require explicit contract review.

Validation: shared checks pass with 295 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests, six hook tests and 11 draft configuration
tests. Rust line coverage is 97.49% (15,897/16,307), above the 80% gate.

### Private composed-resource pending-delivery acceptance

Two tests feed actual protected trust loading and injected credential resolution
through the existing pair-permission pending-delivery guard. Denial/cancellation
skip composition even with a missing trust file. After a synthetic retry, missing
or changed trust, an empty password and a mismatched identity block without send,
clock use or attempt consumption. Reopens preserve attempt two, frozen message
bytes, provider binding and checkpoint. Restored trust and a rotated password
recover the same pending message; another reopen verifies it is no longer due.

No production behavior, dependency, default or public contract changes. Outcomes
come from synthetic callbacks; this is not SMTP delivery evidence. ADR 0019 records
the new acceptance evidence alongside the existing resource-boundary limits.

Next: composed-resource resolution with bounded supervised transport acceptance.
Installed configuration mapping and activation still require explicit review.

Validation: shared checks pass with 297 Rust tests plus one ignored helper, custom
process acceptance, eight SQLite tests, six hook tests and 11 draft configuration
tests. Rust line coverage is 97.49% (16,141/16,556), above the 80% gate.

### Composed resources in supervised transport acceptance

The custom loopback STARTTLS retry/lost-result scenarios now load protected owned
DER files and resolve injected credentials through provider_resolution. Missing
trust and an empty password block before child creation; maintenance stays skipped.
Reopen checks preserve attempt two, frozen bytes, provider binding and checkpoint.
Changed endpoints remain blocked, then restored trust and a rotated password recover
through bounded supervised transport with exact AUTH/DATA assertions. Completed
ordinary work stays complete, and the audit contains only actual transport attempts.

This extends existing acceptance scenarios rather than adding ordinary unit tests.
No external network, cluster, mailbox, dependency, default or product contract changes.
ADR 0019 distinguishes loopback evidence from installed operation.

Next: remaining private resource/configuration mapping preparation. Public parser,
CLI/config/event baselines and runtime activation still require explicit review.

Validation: shared checks pass with 297 Rust tests plus one ignored helper, the
expanded custom process acceptance, eight SQLite tests, six hook tests and 11 draft
configuration tests. Rust line coverage is 97.49% (16,140/16,556), above the 80% gate.
