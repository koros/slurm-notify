# ADR 0017: Private owned trust-set loading

Status: private experiment; no installed loader or approved configuration contract.

## Decision

Add test-only `trust_snapshot` to load caller-selected DER files into owned,
zeroizing snapshots and validate their exact bytes using the existing private
SMTP trust fingerprint function. No dependency or runtime default changes.

Preflight a nonempty set of at most 32 paths and reject duplicate paths before
opening files. Each file uses ADR 0016's protected descriptor reader, including
its private 0700 parent/0600 file policy and metadata rechecks. A caller-selected
read ceiling reduces the remaining 65,536-byte set budget after each snapshot;
an oversized next file reads at most the remaining budget plus one sentinel byte.
No individual file can exceed the original reader ceiling. Exhausted budgets stop
before opening another file. The retained set never exceeds 65,536 bytes.

Validate the complete set as DER trust anchors and reject duplicate certificate
contents even when the paths differ. Publish owned roots and an order-independent
fingerprint only after every read and validation succeeds. Partial sets drop on
failure. Errors contain static categories, without paths, certificate text or I/O
messages. Neither snapshots nor the returned set implements Debug/Clone.

## Evidence and limits

Three tests cover real temporary DER fixtures, reordered reopen, source rotation,
removal after successful loading, duplicate content, malformed DER, count and path
preflight, exact aggregate ceilings and oversized/exhausted budgets. The byte reader
also verifies a reduced ceiling stops at its sentinel and rejects invalid ceilings.
No scheduler, SMTP or external network access is required.

Roots are the exact bytes held by the returned set; later source rotation does not
change that set or fingerprint. Loading multiple files is sequential and does not
promise a transactional snapshot across their sources. ADR 0016's metadata races,
filesystem deadline limitations and platform scope still apply. Strict private
file permissions need product UX review before supporting public trust paths.

The owned set does not parse configuration, resolve ambient trust or credentials,
construct a provider, or activate monitoring. It does not define PEM support or
installed root-selection behavior. The returned fingerprint is evidence of content
identity, not evidence that any SMTP endpoint was contacted or authenticated.

## Next

Prepare private credential/resource resolution within bounded ownership rules.
Installed configuration mapping and activation still require explicit public
contract review.
