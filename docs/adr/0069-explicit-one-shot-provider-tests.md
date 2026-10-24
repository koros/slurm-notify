# ADR 0069: Explicit one-shot provider tests

Date: 2026-10-24
Status: implemented.

`test <provider-name> --config <absolute-path>` authorizes one synthetic request to
that alias. Options may surround command words; the alias follows test. Help is
resource-free. Read one protected config/resource snapshot, resolve effective user,
and validate configured resource metadata. No scheduler execution, SQLite state,
permission enrollment, background process or automatic retry occurs.

Preview the selected safe alias/type and synthetic content, flushing stdout before
network access. Webhook test JSON uses the separate TEST payload baseline; it
contains no real cluster/job data. SMTP uses subject `Slurm Notify test` and the
body `Synthetic Slurm Notify test.` / `No scheduler data.` with a new random opaque
message identity and current Date. SMTP requires the existing exact current
rendering/provider pair before credential lookup. Testing bypasses route event
selection deliberately, but never destination/resource permissions.

Reuse selected same-executable helpers, bounded secret stdin, verified TLS,
redirect/proxy prohibition and existing parent timeout/output/cleanup handling.
A known acknowledgment is required for exit 0; otherwise exit 1 conservatively
reports potentially unknown acceptance and no automatic retry. Invalid resources
or alias exit 2; SMTP pair denial exits 3. Success is provider acknowledgment,
not human receipt. Public monitoring event schemas and durable semantics do not
change; no dependency/config/default changes are introduced.

Installed acceptance covers grammar/help, unknown alias, controlled HTTPS 503 and
204 with exactly one request, preview bytes matching transmitted bytes, SMTP pair
denial before credentials, missing credentials and verified STARTTLS submission.
Tests have no Slurm executable PATH and confirm no state creation. All resources
and credentials are synthetic loopback fixtures.
