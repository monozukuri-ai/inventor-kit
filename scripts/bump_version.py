"""Update Cargo.toml and all release locks, or check their consistency.

Run `python scripts/bump_version.py X.Y.Z`, or omit the version after editing
only [workspace.package].version. Use --check [--tag vX.Y.Z] without modifying
files. Requires Python 3.11+, Cargo and uv; --offline uses cached dependencies.
No commit, tag, push or publication is performed.
"""
import argparse
import re
import subprocess
import sys
import tomllib

from versioning import ROOT, check_versions, project_version, validate_version

MANAGED_FILES = ('Cargo.toml', 'Cargo.lock', 'fuzz/Cargo.lock', 'uv.lock')


def update_manifest(text, version):
    section = ''
    lines = text.splitlines(keepends=True)
    changed = 0
    for index, line in enumerate(lines):
        header = re.fullmatch(r'\s*\[([^\]]+)\]\s*(?:#.*)?', line)
        if header:
            section = header[1]
        elif section == 'workspace.package':
            lines[index], count = re.subn(
                r'^(\s*version\s*=\s*)([\"\']).*?\2',
                lambda m: f'{m[1]}{m[2]}{version}{m[2]}', line)
            changed += count
    if changed != 1:
        raise ValueError('Expected one scalar [workspace.package].version in Cargo.toml')
    return ''.join(lines)


def locked_dependencies(data, *, python=False):
    packages = tomllib.loads(data.decode('utf-8'))['package']
    return [p for p in packages if (p['name'] != 'inventor-kit' if python else 'source' in p)]


def bump_version(root, version=None, *, offline=False):
    version = validate_version(project_version(root) if version is None else version)
    originals = {name: (root/name).read_bytes() for name in MANAGED_FILES}
    updated = update_manifest(originals['Cargo.toml'].decode('utf-8'), version)
    commands = [
        ['cargo', 'update', '--workspace'],
        ['cargo', 'update', '--workspace', '--manifest-path', 'fuzz/Cargo.toml'],
        ['uv', 'lock', '--python', sys.executable],
    ]
    try:
        (root/'Cargo.toml').write_bytes(updated.encode('utf-8'))
        for command in commands:
            subprocess.run(command + (['--offline'] if offline else []), cwd=root, check=True)
        for name in MANAGED_FILES[1:]:
            if locked_dependencies(originals[name], python=name == 'uv.lock') != locked_dependencies(
                    (root/name).read_bytes(), python=name == 'uv.lock'):
                raise ValueError(f'Unexpected dependency changes in {name}; update dependencies separately')
        check_versions(root)
    except BaseException:
        # Preserve the exact starting bytes, including unrelated edits and CRLF.
        for name, data in originals.items():
            (root/name).write_bytes(data)
        raise
    return version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('version', nargs='?', help='stable X.Y.Z; defaults to workspace.package.version')
    parser.add_argument('--check', action='store_true', help='read-only version/lock validation')
    parser.add_argument('--tag', default='', help='require this exact release tag (with --check)')
    parser.add_argument('--offline', action='store_true', help='refresh locks using cached dependencies')
    args = parser.parse_args()
    if args.check and (args.version or args.offline):
        parser.error('--check cannot be combined with a version or --offline')
    if args.tag and not args.check:
        parser.error('--tag requires --check')
    try:
        version = check_versions(ROOT, args.tag) if args.check else bump_version(ROOT, args.version, offline=args.offline)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'Version update/check failed: {error}\n')
    print(f'Version and locks are consistent: {version}')


if __name__ == '__main__':
    main()
