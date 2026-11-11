#!/usr/bin/env python3
"""Collect complete child profiles; empty runtime placeholders contain no counters."""
from pathlib import Path


def collect(target, fallback):
    empty = 0
    for profile in list(target.glob('*.profraw')) + list(fallback.glob('default_*.profraw')):
        # LLVM can create a file before writing its header/counters. A deliberately
        # killed child may leave only that empty placeholder. Never discard a
        # nonempty file: llvm-profdata must reject any corrupt/truncated contents.
        if profile.stat().st_size == 0:
            profile.unlink()
            empty += 1
        elif profile.parent == fallback:
            destination = target / profile.name
            if destination.exists():
                raise FileExistsError('coverage profile destination exists')
            profile.rename(destination)
    return empty


if __name__ == '__main__':
    count = collect(Path('target/llvm-cov-target'), Path('crates/notify-cli'))
    print(f'Coverage collection: removed {count} empty runtime placeholders; all nonempty profiles retained.')
