"""Version/complete artifact gate for an explicit GitHub Release publication."""
import argparse
import json
from pathlib import Path
import tomllib
import urllib.error
import urllib.request

from check_distribution import check

ROOT = Path(__file__).resolve().parents[1]


def check_tag(tag):
    version = tomllib.loads((ROOT / 'pyproject.toml').read_text())['project']['version']
    for crate in ('inventor-core', 'inventor-py'):
        if tomllib.loads((ROOT/'crates'/crate/'Cargo.toml').read_text())['package']['version'] != version:
            raise ValueError('Python and Rust package versions differ')
    if tag and tag != 'v' + version:
        raise ValueError(f'Release tag must be v{version}')
    return version


def artifact_platform(name):
    if name.endswith('.tar.gz'):
        platform = 'sdist'
    elif name.endswith('-win_amd64.whl'):
        platform = 'windows-x86_64'
    elif '-macosx_' in name and name.endswith('_arm64.whl'):
        platform = 'macos-arm64'
    elif '-macosx_' in name and name.endswith('_x86_64.whl'):
        platform = 'macos-x86_64'
    elif '-manylinux' in name and name.endswith('_x86_64.whl'):
        platform = 'linux-x86_64'
    else:
        raise ValueError('Unexpected release artifact: ' + name)
    return platform


def artifact_set(paths):
    expected = {'linux-x86_64', 'windows-x86_64', 'macos-arm64', 'macos-x86_64', 'sdist'}
    found = set()
    reports = []
    for path in paths:
        name = path.name
        platform = artifact_platform(name)
        if platform in found:
            raise ValueError('Duplicate release platform: ' + platform)
        found.add(platform)
        reports.append(check(path))  # Registry-only source lock; no bypass flags.
    if found != expected:
        raise ValueError('Incomplete release artifact set: ' + ', '.join(sorted(expected - found)))
    return reports


def viewer_reports(paths, artifacts):
    """Require installed viewer evidence for every released artifact/interpreter."""
    expected = {}
    for artifact in artifacts:
        name = Path(artifact['file']).name
        target = artifact_platform(name)
        for python in (('3.11',) if target == 'sdist' else ('3.11', '3.12')):
            expected[name, python] = (artifact['sha256'], 'linux-x86_64' if target == 'sdist' else target)
    found = set()
    for path in paths:
        report = json.loads(path.read_text(encoding='utf-8'))
        key = (report['artifact']['file'], '.'.join(report['python'].split('.')[:2]))
        if key not in expected or key in found:
            raise ValueError('Unexpected or duplicate viewer qualification')
        sha256, platform = expected[key]
        if report['artifact']['sha256'] != sha256 or report['platform'] != platform:
            raise ValueError('Viewer qualification artifact/platform mismatch')
        if (report['schema_version'] != 1 or report['status'] != 'passed'
                or report['dependency_mode'] != 'published' or report['pip_check'] != 'passed'
                or report['interpreter_shutdown'] != 'passed' or report['current_state_verified'] is not False):
            raise ValueError('Viewer qualification did not pass with published dependencies')
        if report['dependencies']['ocp-tessellate'] != '3.5.1':
            raise ValueError('Viewer qualification used an unexpected tessellator')
        cases = report['cases']
        if set(cases) != {'part', 'assembly', 'partial_assembly'}:
            raise ValueError('Missing viewer qualification scenario')
        for case, counts in [('part', (1, 1, 0)), ('assembly', (1, 1, 0)), ('partial_assembly', (5, 7, 2))]:
            result = cases[case]
            if (tuple(result[k] for k in ('displayed_instances', 'occurrences', 'omissions')) != counts
                    or result['mesh_buffers_fetched'] <= 0 or result['shutdown'] != 'passed'):
                raise ValueError('Viewer qualification scenario failed')
        found.add(key)
    if found != set(expected):
        raise ValueError('Incomplete viewer qualification: expected all four wheels on Python 3.11/3.12 and rebuilt sdist')
    return len(found)


def pypi_conflicts(version, reports):
    try:
        with urllib.request.urlopen(f'https://pypi.org/pypi/inventor-kit/{version}/json', timeout=30) as response:
            files = json.load(response)['urls']
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
        return
    expected = {Path(r['file']).name: r['sha256'] for r in reports}
    for file in files:
        if expected.get(file['filename']) != file['digests']['sha256']:
            raise ValueError('Existing PyPI artifact differs: ' + file['filename'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', default='')
    parser.add_argument('--dist', type=Path)
    parser.add_argument('--viewer-reports', type=Path, help='Require viewer qualification reports for the complete artifact set')
    parser.add_argument('--check-pypi', action='store_true')
    args = parser.parse_args()
    version = check_tag(args.tag)
    reports = artifact_set(sorted(args.dist.iterdir())) if args.dist else None
    if args.viewer_reports:
        if reports is None:
            parser.error('--viewer-reports requires --dist')
        viewer_reports(sorted(args.viewer_reports.glob('*.json')), reports)
    if args.check_pypi:
        if reports is None:
            parser.error('--check-pypi requires --dist')
        pypi_conflicts(version, reports)
    print(json.dumps(dict(version=version, artifacts=reports)))


if __name__ == '__main__':
    main()
