# ADR 0071: Relocated local MVP workflow acceptance

Date: 2026-10-25
Status: controlled installed acceptance implemented; external support gates remain.

Copy the built executable into a temporary user-owned installation directory and
run outside the checkout with synthetic Slurm executables and loopback providers.
Exercise terminal draft review/enrollment, config validation and doctor before any
state exists. Then run mixed SMTP/HTTPS monitoring: SMTP accepts once, HTTPS returns
503, a fresh process retries identical frozen UUID/JSON/idempotency bytes and gets
204, and a third process does not replay either completed destination. Inspect the
closed historical state. Independent checks confirm one SMTP attempt, three total
attempts, two durable destination rows and shared event identity/frozen bytes.

This proves local composition and executable/helper relocation against controlled
verified TLS. Existing one-shot provider acceptance covers explicit synthetic tests
separately. No runtime/config/dependency defaults or compatibility baselines change.
The getting-started guide documents source build, protected draft review, deliberate
settings, diagnostics, explicit sends, foreground operation, restart and closed
backup/upgrade precautions. Support/security documentation separates these results
from authorized real-cluster/provider, platform, artifact and release acceptance.
