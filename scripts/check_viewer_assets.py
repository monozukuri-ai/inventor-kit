"""Check bundled viewer assets against build inputs and output hashes."""
import hashlib
import json
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]


def check_bundle(read, names, prefix, source_prefix=None):
    manifest = json.loads(read(prefix+'manifest.json'))
    if manifest['schema_version'] != 1:
        raise ValueError('Unknown viewer asset manifest')
    outputs = manifest['outputs']
    if not {'index.html', 'THIRD_PARTY_LICENSES.txt'} <= outputs.keys() or not any(n.endswith('.js') for n in outputs) or not any(n.endswith('.css') for n in outputs):
        raise ValueError('Incomplete viewer asset manifest')
    for table, base in [(outputs, prefix)] + ([(manifest['inputs'], source_prefix)] if source_prefix is not None else []):
        if not table:
            raise ValueError('Empty viewer build manifest')
        for name, digest in table.items():
            path = PurePosixPath(name)
            if path.is_absolute() or '..' in path.parts or '\\' in name:
                raise ValueError('Unsafe viewer asset path')
            full = base+name
            if full not in names or hashlib.sha256(read(full)).hexdigest() != digest:
                raise ValueError(f'Missing or stale viewer asset/input: {full}')
    actual = {n[len(prefix):] for n in names if n.startswith(prefix)}
    if actual != set(outputs) | {'manifest.json'}:
        raise ValueError('Unlisted viewer asset')
    return len(outputs)


def main():
    static = ROOT/'python/inventor_kit/viewer/static'
    source = ROOT/'viewer'
    manifest = json.loads((static/'manifest.json').read_text())
    names = {p.relative_to(ROOT).as_posix() for p in static.rglob('*') if p.is_file()}
    names.update('viewer/'+n for n in manifest['inputs'] if (source/n).is_file())
    count = check_bundle(lambda n: (ROOT/n).read_bytes(), names, 'python/inventor_kit/viewer/static/', 'viewer/')
    print(f'Viewer assets passed: {count} files; source and bundle hashes match')


if __name__ == '__main__':
    main()
