#!/usr/bin/env python3
"""Opt-in download of the separate, hash-pinned M5 assembly corpus."""
import argparse
import hashlib
import json
from pathlib import Path
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dest', type=Path, default=ROOT / 'fixtures/public')
    args = parser.parse_args()
    rows = json.loads((ROOT / 'fixtures/assembly-manifest.json').read_text())
    for row in rows:
        relative = Path(row['file'])
        if relative.is_absolute() or '..' in relative.parts:
            raise ValueError('Manifest path leaves destination')
        target = args.dest / relative
        if target.exists() and hashlib.sha256(target.read_bytes()).hexdigest() == row['sha256']:
            print(f'cached {relative}')
            continue
        with urllib.request.urlopen(row['url'], timeout=60) as response:
            data = response.read(row['bytes'] + 1)
        if len(data) != row['bytes'] or hashlib.sha256(data).hexdigest() != row['sha256']:
            raise ValueError(f'Fixture mismatch: {relative}')
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        print(f'fetched {relative}')


if __name__ == '__main__':
    main()
