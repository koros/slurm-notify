# ADR 0066: Webhook-only foreground composition

Date: 2026-10-24

Status: implemented with installed failure and restart acceptance.

## Decision

Permit a prepared foreground plan with no SMTP providers. Reuse the existing
webhook-only public transaction path for polling, route admission, frozen UUID/JSON
and endpoint revisions. Keep mixed/SMTP preparation checks unchanged; never pass
empty SMTP bindings into the mixed storage path or fabricate an SMTP provider.
Webhook-only configs need no SMTP message limit or permission list. Existing schema
validation remains authoritative, including explicit optional SMTP fields if present.

The same owned session, scheduler bounds, cadence, signal handling, maintenance,
helper selection and delivery worker apply. No dependencies, defaults or persistence
schema change. Existing historical state and frozen work remain compatible. Global
and start help deliberately remove the former SMTP-required restriction.

## Validation and limits

Installed synthetic scheduler acceptance covers missing resources before state,
failed loopback dispatch, SIGINT/SIGTERM and reopen with identical frozen public
bytes and no SMTP messages. Existing private webhook transactions cover rollback,
admission and endpoint revision guards. Successful installed HTTPS dispatch remains
a separate acceptance requirement. No real-cluster/provider or supported-platform
claim follows. ADR 0065's SMTP-required restriction is superseded by this decision.
