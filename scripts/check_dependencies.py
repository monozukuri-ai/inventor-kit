"""Check the resolved Python-extension graph for a single registry ACIS core."""
import argparse
import json
import subprocess
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def check_python_lock(root=ROOT):
    project = tomllib.loads((root/'pyproject.toml').read_text(encoding='utf-8'))['project']
    packages = tomllib.loads((root/'uv.lock').read_text(encoding='utf-8'))['package']
    own = [p for p in packages if p['name'] == 'inventor-kit']
    shared = [p for p in packages if p['name'] == 'cq-acis']
    if len(own) != 1 or own[0]['version'] != project['version']:
        raise ValueError('uv.lock project version is stale; refresh after dependency publication')
    if (len(shared) != 1 or shared[0].get('source', {}).get('registry') != 'https://pypi.org/simple'
            or tuple(map(int, shared[0]['version'].split('.'))) < (0, 3, 10)
            or tuple(map(int, shared[0]['version'].split('.'))) >= (0, 4, 0)):
        raise ValueError('uv.lock must resolve compatible published cq-acis >=0.3.10,<0.4')


def check_graph(metadata, root_name='inventor-py', allow_local_bridge=False, allow_local_core=False):
    packages = {p['id']: p for p in metadata['packages']}
    roots = [p['id'] for p in packages.values() if p['name'] == root_name]
    if len(roots) != 1:
        raise ValueError(f'Expected one root package: {root_name}')
    nodes = {n['id']: n['dependencies'] for n in metadata['resolve']['nodes']}
    visited, pending = set(), list(roots)
    while pending:
        current = pending.pop()
        if current in visited:
            continue
        visited.add(current)
        pending.extend(nodes[current])
    result = {}
    for name in ('acis-core', 'acis-py-bridge'):
        found = [packages[i] for i in visited if packages[i]['name'] == name]
        if len(found) != 1 or found[0]['version'] != '0.3.10':
            raise ValueError(f'Expected exactly one {name} 0.3.10 in the resolved extension graph')
        p = found[0]
        registry = p['source'] == 'registry+https://github.com/rust-lang/crates.io-index'
        if not registry and not (((name == 'acis-py-bridge' and allow_local_bridge) or (name == 'acis-core' and allow_local_core)) and p['source'] is None):
            raise ValueError(f'{name} must come from crates.io; observed {p["source"] or p["manifest_path"]}')
        result[name] = {'version': p['version'], 'source': p['source'], 'manifest_path': p['manifest_path']}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', default='inventor-py')
    parser.add_argument('--allow-local-bridge', action='store_true', help='Explicit development mode; does not establish registry-only readiness')
    parser.add_argument('--allow-local-core', action='store_true', help='Explicit development core override; publication still required')
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    if not (args.allow_local_bridge or args.allow_local_core):
        check_python_lock()
    command = ['cargo', 'metadata', '--locked', '--format-version', '1']
    if args.offline:
        command.append('--offline')
    metadata = json.loads(subprocess.check_output(command))
    print(json.dumps(check_graph(metadata, args.root, args.allow_local_bridge, args.allow_local_core), indent=2))


if __name__ == '__main__':
    main()
