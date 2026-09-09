"""Fail before tests if any declared native regression or holdout file is missing."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]


def verify(fixtures, manifests=None):
    manifests = manifests or [ROOT / 'fixtures/manifest.json', ROOT / 'fixtures/assembly-manifest.json']
    rows = []
    for manifest in manifests:
        seen = set()
        for row in json.loads(Path(manifest).read_text()):
            name = row['file']
            relative = PurePosixPath(name)
            if relative.is_absolute() or '..' in relative.parts or '\\' in name or '\x00' in name or name in seen:
                raise ValueError(f'Unsafe/duplicate corpus path: {name}')
            seen.add(name)
            path = (fixtures / name).resolve()
            if not path.is_relative_to(fixtures.resolve()) or not path.is_file():
                raise ValueError(f'Missing or escaped fixture: {name}')
            if path.stat().st_size != row['bytes'] or hashlib.sha256(path.read_bytes()).hexdigest() != row['sha256']:
                raise ValueError(f'Fixture size/hash mismatch: {name}')
            rows.append(dict(file=name, split=row['split'], sha256=row['sha256']))
        if not seen:
            raise ValueError('Empty corpus manifest')
    return dict(status='passed', fixtures=len(rows), holdouts=sum(r['split'] == 'holdout' for r in rows), files=rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'fixtures/public')
    args = parser.parse_args()
    report = verify(args.fixtures)
    print(json.dumps({k: v for k, v in report.items() if k != 'files'}))


if __name__ == '__main__':
    main()
