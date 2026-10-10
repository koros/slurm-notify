#!/usr/bin/env python3
"""Check index blobs, without printing potential credentials or modifying files.

This small pattern check is not a comprehensive secret-scanning service.
"""
import argparse
import re
import subprocess
from pathlib import PurePosixPath

PATTERNS = [
    re.compile(rb"-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----"),
    re.compile(rb"gh[pousr]_[A-Za-z0-9]{30,}"),
    re.compile(rb"github_pat_[A-Za-z0-9_]{40,}"),
    re.compile(rb"xox[baprs]-[A-Za-z0-9-]{20,}"),
    re.compile(rb"AKIA[0-9A-Z]{16}"),
]


def git(*args):
    return subprocess.check_output(["git", *args])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--all", action="store_true", help="check every tracked index blob")
    options = parser.parse_args()
    if options.all:
        names = git("ls-files", "-z")
    else:
        names = git("diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z")
    failures = []
    for raw_name in names.split(b"\0"):
        if not raw_name:
            continue
        name = raw_name.decode("utf-8", errors="surrogateescape")
        path = PurePosixPath(name)
        if path.name == ".env" or (path.name.startswith(".env.") and path.name != ".env.example"):
            failures.append((name, "environment credential file must not be tracked"))
            continue
        if path.name in {"id_rsa", "id_ed25519", "id_ecdsa"}:
            failures.append((name, "private key file must not be tracked"))
            continue
        # Reject symlinks/submodules before staged snapshot checks follow paths.
        entry = git("ls-files", "--stage", "-z", "--", name).split(b" ", 1)[0]
        if entry not in {b"100644", b"100755"}:
            failures.append((name, "only regular tracked files are supported by the staged check"))
            continue
        data = git("show", ":" + name)
        if any(pattern.search(data) for pattern in PATTERNS):
            failures.append((name, "possible credential detected"))
        if b"\0" in data:
            continue
        for number, line in enumerate(data.splitlines(), 1):
            if re.search(rb"[ \t]+$", line):
                # Two spaces are an intentional CommonMark hard line break.
                if path.suffix.lower() == ".md" and line.endswith(b"  ") and not line[:-2].endswith((b" ", b"\t")):
                    continue
                failures.append((name, f"trailing whitespace on line {number}"))
                break
        if any(line in (b"<<<<<<<", b"=======", b">>>>>>>") or line.startswith((b"<<<<<<< ", b">>>>>>> ")) for line in data.splitlines()):
            failures.append((name, "unresolved merge marker"))
    for name, reason in failures:
        print(f"{name!r}: {reason}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
