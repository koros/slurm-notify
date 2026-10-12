# ADR 0026: private webhook draft composition

Status: private experiment; no installed parser/runtime or approved contract.

## Decision

Compose a fully validated webhook-only draft into owned observation policy,
numeric controls, compiled route selection, storage-path metadata and resolved
webhook resources. Full shape/type/reference validation and provider-scope checks
precede injected identity or URL/header lookup. Reject SMTP-containing drafts with
a static UnsupportedProvider result before any lookup; do not silently ignore
SMTP or weaken permission/trust policy.

Identity lookup precedes provider resolution. The existing adapters recheck returned
identity and resource bytes. No partially resolved plan is returned on failure;
owned zeroizing metadata/resources drop on early return. Configuration source bytes
remain caller-owned. Lookups retain caller-managed allocation/deadline policy; this
composition does not create ambient environment access or an effective-user adapter.

The plan's private routed-poll method builds the request from validated cluster,
identity, initial-observation policy and mapped query timeout/output bounds.
Programs, history window and observation time remain explicit caller inputs.
State must be supplied by an owner; the adapter does not open the stored path or
validate its filesystem semantics. Existing atomic routed persistence remains the
commit boundary. Resource resolution is eager for all configured webhooks, including
unselected ones; per-resource bounds and the provider ceiling bound retained work.
No process/network dispatch, foreground loop or public payload is introduced.

## Evidence and limits

Five tests verify malformed/policy-invalid and unsupported mixed drafts fail before
lookup; identity failure prevents resource lookup; invalid URL stops before header
lookup; missing resources and late unselected-provider failure return no partial plan; source-independent metadata,
resource frame bytes and numeric/routing mapping; and durable injected polling for
both emit/suppress policies with exact selected outbox work after reopen. Queries
assert current-user arguments and mapped budgets. The SQLite fixture uses a local
owned temporary path; draft paths are not opened by preparation.

The frame test uses synthetic payload bytes. It does not establish public webhook
IDs, frozen payload persistence, disclosure approval or receipt. Polling uses
synthetic Slurm records and no actual cluster. SMTP profile/trust/permission
composition, installed parsing, effective-user/explicit environment adapters,
blocked-batch scheduling and foreground cancellation remain outstanding. Public
contract baselines and installed activation still require explicit review.

## Next

Compose validated mixed drafts into private bound SMTP rendering/provider and exact
permission plans, with trust/credential failure ordering and restart acceptance.
