"""Fail before tests if any declared native regression or holdout file is missing."""
import argparse
import hashlib
import json
from pathlib import Path

from corpus_manifest import load_corpus

ROOT = Path(__file__).resolve().parents[1]


def verify(fixtures, manifests=None):
    fixtures = Path(fixtures).resolve()
    if manifests is None:
        from drawing_corpus import load_drawings
        load_drawings()
    manifests = manifests if manifests is not None else [ROOT / 'fixtures' / name for name in
        ('manifest.json', 'assembly-manifest.json', 'drawing-manifest.json')]
    canonical, _ = load_corpus(manifests)
    rows = []
    for row in canonical:
        name = row['file']
        path = (fixtures / name).resolve()
        if not path.is_relative_to(fixtures) or not path.is_file():
            raise ValueError(f'Missing or escaped fixture: {name}')
        if path.stat().st_size != row['bytes'] or hashlib.sha256(path.read_bytes()).hexdigest() != row['sha256']:
            raise ValueError(f'Fixture size/hash mismatch: {name}')
        rows.append(dict(file=name, split=row['split'], sha256=row['sha256']))
    return dict(status='passed', fixtures=len(rows), holdouts=sum(r['split'] == 'holdout' for r in rows), files=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'fixtures/public')
    args = parser.parse_args()
    report = verify(args.fixtures)
    print(json.dumps({k: v for k, v in report.items() if k != 'files'}))


if __name__ == '__main__':
    main()
