"""Opt-in drawing fixture downloads, including canonical existing selections."""
import argparse
import hashlib
from pathlib import Path
import tempfile

from corpus_manifest import ROOT
from drawing_corpus import load_drawings
from fixture_download import download


def fetch(dest, fixtures_dir=ROOT / 'fixtures'):
    data, lookup = load_drawings(fixtures_dir)
    rows = {r['file']: r for r in data['assets']}
    for entry in data['drawings']:
        source = lookup[entry['fixture']['manifest'], entry['fixture']['file']]
        rows[source['file']] = source
    dest = Path(dest).resolve()
    for row in rows.values():
        target = (dest / row['file']).resolve()
        if not target.is_relative_to(dest):
            raise ValueError('Fixture target escapes destination')
        if target.is_file() and target.stat().st_size == row['bytes'] and hashlib.sha256(target.read_bytes()).hexdigest() == row['sha256']:
            print(f"cached {row['file']}")
            continue
        payload = download(row['url'], max_bytes=row['bytes'], timeout=60)
        if len(payload) != row['bytes'] or hashlib.sha256(payload).hexdigest() != row['sha256']:
            raise ValueError(f"Fixture size/hash mismatch: {row['file']}")
        target.parent.mkdir(parents=True, exist_ok=True)
        # A unique file also avoids clashes between independent fetch processes.
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as output:
                temporary = Path(output.name)
                output.write(payload)
            temporary.replace(target)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
        print(f"fetched {row['file']}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dest', type=Path, default=ROOT / 'fixtures/public')
    args = parser.parse_args()
    fetch(args.dest)


if __name__ == '__main__':
    main()
