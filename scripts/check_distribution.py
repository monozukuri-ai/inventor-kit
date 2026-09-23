"""Check inventor-kit wheels/sdists, including dependency and source boundaries."""
from email.parser import BytesParser
import base64
import csv
import io
from pathlib import Path, PurePosixPath
import argparse
import hashlib
import json
import posixpath
import tarfile
import tomllib
import zipfile
from check_viewer_assets import check_bundle
from check_license import DISTRIBUTION_LICENSE, check_archive_licenses, notice_bundle

ROOT = Path(__file__).resolve().parents[1]


def check_metadata(meta, version):
    if (meta['Name'], meta['Version'], meta['License-Expression']) != ('inventor-kit', version, DISTRIBUTION_LICENSE):
        raise ValueError('Unexpected package name, version or license')
    if meta['Requires-Python'] != '>=3.11':
        raise ValueError('Distribution must require Python >=3.11')
    requirements = [r.replace(' ', '') for r in meta.get_all('Requires-Dist', []) if r.startswith('cq-acis')]
    if len(requirements) != 1 or set(requirements[0].removeprefix('cq-acis').split(',')) != {'>=0.3.8', '<0.4'}:
        raise ValueError('Distribution does not require the compatible cq-acis API series')
    viewer = [r.replace(' ', '').replace("'", '"') for r in meta.get_all('Requires-Dist', []) if r.startswith('ocp-tessellate')]
    if 'viewer' not in meta.get_all('Provides-Extra', []) or viewer != ['ocp-tessellate==3.5.1;extra=="viewer"']:
        raise ValueError('Distribution is missing the optional viewer dependency contract')


def check(path, *, allow_unpublished_bridge=False, allow_unpublished_core=False):
    version = tomllib.loads((ROOT / 'pyproject.toml').read_text())['project']['version']
    if path.name.endswith('.whl'):
        if not path.name.startswith(f'inventor_kit-{version}-'):
            raise ValueError('Wheel filename package/version mismatch')
        with zipfile.ZipFile(path) as archive:
            if len(archive.namelist()) != len(set(archive.namelist())) or archive.testzip() is not None:
                raise ValueError('Duplicate or corrupt wheel members')
            contents = {n: archive.read(n) for n in archive.namelist() if not n.endswith('/')}
        if '-cp310-abi3-' not in path.name or not any(n.startswith('inventor_kit/_inventor.') and n.endswith(('.so', '.pyd')) for n in contents):
            raise ValueError('Missing ABI3 native extension')
        package_prefix = 'inventor_kit/'
        viewer_sources = None
        for name in ('drawing.py', 'drawing_output.py', 'viewer/drawing.py', 'assembly.py', 'assembly_step.py', 'conversion.py', 'cli.py', '_cli_worker.py', 'limits.py', 'capabilities.json'):
            if 'inventor_kit/' + name not in contents:
                raise ValueError(f'Missing assembly Python API: {name}')
        metadata = [v for k, v in contents.items() if k.endswith('.dist-info/METADATA')]
        license_prefix = f'inventor_kit-{version}.dist-info/licenses/'
        wheel_metadata = [v for k, v in contents.items() if k.endswith('.dist-info/WHEEL')]
        python_tag, abi_tag, platform_tag = path.stem.rsplit('-', 3)[1:]
        expected_tags = {f'{p}-{a}-{t}' for p in python_tag.split('.') for a in abi_tag.split('.') for t in platform_tag.split('.')}
        if len(wheel_metadata) != 1 or set(BytesParser().parsebytes(wheel_metadata[0]).get_all('Tag', [])) != expected_tags:
            raise ValueError('Wheel tag metadata/filename mismatch')
        records = [k for k in contents if k.endswith('.dist-info/RECORD')]
        if len(records) != 1:
            raise ValueError('Expected one wheel RECORD')
        listed = set()
        for record_name, digest, size in csv.reader(io.StringIO(contents[records[0]].decode())):
            # Windows builders may use backslashes in RECORD, while ZIP member
            # names use forward slashes. Normalize before matching/deduplicating,
            # including when Windows wheels are checked on Linux before upload.
            name = record_name.replace('\\', '/')
            if name in listed:
                raise ValueError(f'Duplicate RECORD member: {record_name!r}')
            if name not in contents:
                raise ValueError(f'Missing RECORD member: {record_name!r}')
            listed.add(name)
            if name == records[0] and not digest and not size:
                continue
            value = 'sha256=' + base64.urlsafe_b64encode(hashlib.sha256(contents[name]).digest()).decode().rstrip('=')
            if digest != value or size != str(len(contents[name])):
                raise ValueError(f'Wheel RECORD digest/size mismatch: {name}')
        if listed != set(contents):
            raise ValueError('Unrecorded wheel members')
    elif path.name.endswith('.tar.gz'):
        if path.name != f'inventor_kit-{version}.tar.gz':
            raise ValueError('Source filename package/version mismatch')
        with tarfile.open(path) as archive:
            contents = {m.name: archive.extractfile(m).read() for m in archive if m.isfile()}
        prefix = f'inventor_kit-{version}/'
        license_prefix = prefix
        package_prefix = prefix+'python/inventor_kit/'
        viewer_sources = prefix+'viewer/'
        for name in ('Cargo.toml', 'Cargo.lock', 'crates/inventor-core/src/analysis.rs', 'crates/inventor-core/src/candidate.rs', 'crates/inventor-core/src/document.rs', 'crates/inventor-core/src/property.rs', 'crates/inventor-core/src/thumbnail.rs', 'crates/inventor-py/src/lib.rs', 'python/inventor_kit/__init__.py', 'python/inventor_kit/document.py', 'python/inventor_kit/geometry.py', 'schemas/vendor-oracle-v1.schema.json', 'schemas/drawing-oracle-v1.schema.json', 'schemas/drawing-scene-v1.schema.json', 'schemas/drawing-report-v1.schema.json'):
            if prefix + name not in contents:
                raise ValueError(f'Missing sdist build input: {name}')
        for name in ('mod.rs', 'inventory.rs', 'profile.rs', 'fields.rs', 'sheet.rs', 'sheets.rs', 'geometry.rs', 'text.rs', 'style.rs', 'scene.rs', 'scene_tests.rs', 'appearance.rs', 'images.rs', 'tests.rs'):
            if prefix + 'crates/inventor-core/src/drawing/' + name not in contents:
                raise ValueError(f'Missing drawing Rust source: {name}')
        for name in ('mod.rs', 'ufrx.rs', 'records.rs', 'matrix.rs', 'resolve.rs', 'tests.rs'):
            if prefix + 'crates/inventor-core/src/assembly/' + name not in contents:
                raise ValueError(f'Missing assembly Rust source: {name}')
        for name in ('drawing.py', 'drawing_output.py', 'viewer/drawing.py', 'assembly.py', 'assembly_step.py', 'conversion.py', 'cli.py', '_cli_worker.py', 'limits.py', 'capabilities.json'):
            if prefix + 'python/inventor_kit/' + name not in contents:
                raise ValueError(f'Missing assembly Python source: {name}')
        for name, content in contents.items():
            if name.endswith('Cargo.toml'):
                # Any remaining path must name a crate within this source archive.
                data = tomllib.loads(content.decode())
                def walk(node):
                    if isinstance(node, dict):
                        if 'path' in node:
                            target = posixpath.normpath(posixpath.join(posixpath.dirname(name), node['path']))
                            if not target.startswith(prefix) or (target not in contents and target + '/Cargo.toml' not in contents):
                                raise ValueError(f'External Cargo path in sdist: {name}: {node["path"]}')
                        for v in node.values():
                            walk(v)
                    elif isinstance(node, list):
                        for v in node:
                            walk(v)
                walk(data)
        lock = tomllib.loads(contents[prefix+'Cargo.lock'].decode())
        for package in ('acis-core', 'acis-py-bridge'):
            entries = [p for p in lock['package'] if p['name'] == package]
            if len(entries) != 1 or entries[0]['version'] != '0.3.8':
                raise ValueError(f'Expected one pinned {package}')
            entry = entries[0]
            registry = entry.get('source') == 'registry+https://github.com/rust-lang/crates.io-index' and len(entry.get('checksum', '')) == 64
            if not registry and not (((package == 'acis-py-bridge' and allow_unpublished_bridge) or (package == 'acis-core' and allow_unpublished_core)) and 'source' not in entry):
                raise ValueError(f'{package} is not locked to crates.io; finish publication and refresh Cargo.lock')
        metadata = [contents[prefix+'PKG-INFO']]
        for crate in ('inventor-core', 'inventor-py'):
            if contents.get(prefix+'crates/'+crate+'/LICENSE') != notice_bundle():
                raise ValueError('Missing or stale crate license bundle: ' + crate)
    else:
        raise ValueError(f'Unexpected artifact: {path}')
    for name in contents:
        parts = PurePosixPath(name).parts
        if any(p in {'internal', 'fixtures', 'reports', 'corpus', '.cargo', '.venv', '.git', 'target', 'fuzz', 'node_modules', 'test-results', 'playwright-report'} for p in parts):
            raise ValueError(f'Development data/configuration leaked into artifact: {name}')
        if '..' in parts or name.startswith('/'):
            raise ValueError(f'Unsafe archive member: {name}')
    if len(metadata) != 1:
        raise ValueError('Expected one distribution metadata file')
    meta = BytesParser().parsebytes(metadata[0])
    check_metadata(meta, version)
    check_archive_licenses(contents, meta, license_prefix)
    for name in ('__init__.py', '__main__.py', 'cli.py', 'scene.py', 'server.py', 'worker.py', 'tessellation.py', 'assembly.py', 'part.py'):
        if package_prefix+'viewer/'+name not in contents:
            raise ValueError(f'Missing viewer Python module: {name}')
    check_bundle(contents.__getitem__, set(contents), package_prefix+'viewer/static/', viewer_sources)
    if contents[package_prefix+'viewer/static/LICENSE.txt'] != notice_bundle(viewer=True):
        raise ValueError('Stale viewer license bundle')
    return {'file': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest(), 'registry_only_required': not (allow_unpublished_bridge or allow_unpublished_core)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifacts', nargs='+', type=Path)
    parser.add_argument('--allow-unpublished-bridge', action='store_true', help='Provisional source validation; publication and registry-only rebuild still required')
    parser.add_argument('--allow-unpublished-core', action='store_true', help='Provisional source validation with a staged development core')
    args = parser.parse_args()
    for path in args.artifacts:
        print(json.dumps(check(path, allow_unpublished_bridge=args.allow_unpublished_bridge, allow_unpublished_core=args.allow_unpublished_core)))


if __name__ == '__main__':
    main()
