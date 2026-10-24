# ADR 0064: Private same-executable provider selection

Date: 2026-10-24

Status: selector/process acceptance and installed webhook/SMTP handlers implemented.
Installed rejection, missing-STARTTLS/no-AUTH and verified SMTP helper submission/
replay acceptance pass. Installed monitoring/store restart and successful webhook
helper send acceptance remain pending.

## Decision

Capture `std::env::current_exe()` once as an absolute regular-file pathname. Build
provider commands from that owned path with exactly two fixed arguments:
`--internal-provider-v1 webhook` or `--internal-provider-v1 smtp-starttls`.
Do not discover helpers through PATH, configuration, or environment overrides.
The supervisor clears the child environment and transfers per-request endpoint,
authorization, public event bytes or SMTP resources through bounded owned stdin.

The private harness accepts only the exact grammar. Webhook serving requires HTTPS;
SMTP serving retains the bound required-STARTTLS codec. Malformed/truncated frames
fail before provider activity. Parent deadlines, bounded output, child cleanup and
sanitized acknowledgment classification remain authoritative.

## Compatibility and limits

This is an internal versioned helper protocol, not a supported user command or Rust
API. Public CLI grammar/help baselines remain unchanged. The installed webhook
handler shares the bounded codec and HTTPS client with malformed-input binary
acceptance. Activating the remaining installed handlers must add acceptance before
monitoring can select them. No dependency or operational timeout defaults change.

Capturing a pathname avoids ambient helper discovery; it does not pin an executable
inode across replacement. The selected image must remain available throughout a
run. Filesystem metadata lookup remains synchronous, as with current resource reads.
No service installation, release, supported-platform claim or real-provider acceptance
is established by these private fixtures.

## Validation

Unit tests check the current image, exact argument arrays, absent environment
selection and rejected relative/missing/directory paths and malformed grammar.
Process acceptance executes both selected helper modes with malformed stdin and
requires supervised failure without a provider request or recursive parent startup.
