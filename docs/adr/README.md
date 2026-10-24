# Architecture decision records

These records preserve decisions and evidence at their original milestones.
References to pending licensing, old toolchains/coverage gates or work that was
then unimplemented are historical. Current licensing is MIT and the Rust coverage
gate is 90%; use [product readiness](../architecture/product-readiness.md) and the
[current plan](../architecture/implementation-plan.md) for present status.

Recent integration evidence: [combined OS foreground lifecycle](0057-private-combined-os-foreground-lifecycle.md),
[OS runtime resources](0056-private-os-runtime-resource-composition.md) and
[active-callback signals](0055-private-active-callback-os-signal-acceptance.md).

Installed foreground decisions: [SMTP/mixed activation](0065-installed-smtp-mixed-foreground-command.md)
and [webhook-only composition](0066-webhook-only-foreground-composition.md).

[Explicit webhook trust and installed HTTPS](0067-explicit-webhook-trust-and-installed-https.md) records controlled TLS acceptance.
