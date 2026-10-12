"""Shared release-version checks; Cargo.toml is the only version definition."""
import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parents[1]
STABLE_VERSION = re.compile(r'(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)')


def read_toml(path):
    return tomllib.loads(path.read_text(encoding='utf-8'))


def validate_version(version):
    if not isinstance(version, str) or not STABLE_VERSION.fullmatch(version):
        raise ValueError('version must be a stable X.Y.Z release without leading zeros')
    return version


def project_version(root=ROOT):
    return validate_version(read_toml(root/'Cargo.toml')['workspace']['package']['version'])


def check_sources(root=ROOT):
    version = project_version(root)
    workspace = read_toml(root/'Cargo.toml')['workspace']
    for member in workspace['members']:
        package = read_toml(root/member/'Cargo.toml')['package']
        if package.get('version') != {'workspace': True}:
            raise ValueError(f'{member} must inherit version.workspace = true')
    project = read_toml(root/'pyproject.toml')['project']
    if 'version' in project or 'version' not in project.get('dynamic', []):
        raise ValueError('Python must use dynamic version metadata from Cargo.toml')
    viewer = json.loads((root/'viewer/package.json').read_text(encoding='utf-8'))
    lock = json.loads((root/'viewer/package-lock.json').read_text(encoding='utf-8'))
    if viewer.get('private') is not True or any('version' in p for p in (viewer, lock, lock['packages'][''])):
        raise ValueError('The private viewer must not duplicate the release version')
    return version


def check_python_version_lock(root=ROOT):
    version = project_version(root)
    project = read_toml(root/'pyproject.toml')['project']
    if 'version' in project or 'version' not in project.get('dynamic', []):
        raise ValueError('Python must use dynamic version metadata from Cargo.toml')
    packages = read_toml(root/'uv.lock')['package']
    own = [p for p in packages if p['name'] == 'inventor-kit']
    # uv can omit a dynamically versioned editable project's version entirely.
    if (len(own) != 1 or own[0].get('version', version) != version
            or own[0].get('source') != {'editable': '.'}):
        raise ValueError('uv.lock project version/source is stale; run scripts/bump_version.py')


def check_cargo_lock(path, expected):
    packages = read_toml(path)['package']
    for name, version in expected.items():
        entries = [p for p in packages if p['name'] == name]
        if (len(entries) != 1 or entries[0]['version'] != version
                or 'source' in entries[0] or 'checksum' in entries[0]):
            raise ValueError(f'{path}: expected one local {name} {version}; run scripts/bump_version.py')


def check_versions(root=ROOT, tag=''):
    version = check_sources(root)
    workspace = read_toml(root/'Cargo.toml')['workspace']
    names = {read_toml(root/member/'Cargo.toml')['package']['name']: version
             for member in workspace['members']}
    check_cargo_lock(root/'Cargo.lock', names)
    fuzz = read_toml(root/'fuzz/Cargo.toml')['package']
    check_cargo_lock(root/'fuzz/Cargo.lock', {'inventor-core': version, fuzz['name']: fuzz['version']})
    check_python_version_lock(root)
    if tag and tag != 'v' + version:
        raise ValueError(f'Release tag must be v{version}')
    return version
