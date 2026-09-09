#!/usr/bin/env python3
"""Opt-in public corpus download; verify both archive and extracted-file hashes."""
from pathlib import Path
import argparse
import hashlib
import io
import urllib.request
import zipfile
from corpus_manifest import load_manifest

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dest', type=Path, default=ROOT / 'fixtures/public')
    parser.add_argument('--cache', type=Path, default=ROOT / 'fixtures/public/.archives')
    parser.add_argument('--split', choices=['all', 'regression', 'holdout'], default='all')
    args = parser.parse_args()
    args.dest.mkdir(parents=True, exist_ok=True)
    args.cache.mkdir(parents=True, exist_ok=True)
    archives = {}
    for item in load_manifest(split=args.split):
        target = args.dest / item['file']
        if target.exists() and hashlib.sha256(target.read_bytes()).hexdigest() == item['sha256']:
            print(f'cached {target.name}')
            continue
        url = item.get('resolved_url', item['url'])
        if 'archive_member' in item:
            sha = item['archive_sha256']
            if sha not in archives:
                cached = args.cache / f'{sha}.zip'
                data = cached.read_bytes() if cached.exists() else urllib.request.urlopen(url, timeout=120).read()
                if hashlib.sha256(data).hexdigest() != sha:
                    raise ValueError(f'Archive SHA-256 mismatch: {url}')
                cached.write_bytes(data)
                archives[sha] = zipfile.ZipFile(io.BytesIO(data))
            data = archives[sha].read(item['archive_member'])
        else:
            data = urllib.request.urlopen(url, timeout=120).read()
        if len(data) != item['bytes'] or hashlib.sha256(data).hexdigest() != item['sha256']:
            raise ValueError(f'Fixture mismatch: {target.name}')
        target.write_bytes(data)
        print(f'fetched {target.name}')


if __name__ == '__main__':
    main()
