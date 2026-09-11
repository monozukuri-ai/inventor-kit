"""Rebuild/install distributions outside checkouts; local release candidates are explicit."""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def installed_viewer(corpus, file='SamplePart.ipt', options=(), parts=1, occurrences=1, omissions=0):
    import queue
    import signal
    import threading
    import time
    from urllib.request import urlopen
    import inventor_kit.viewer
    assert Path(inventor_kit.viewer.__file__).resolve().is_relative_to(Path(sys.prefix).resolve())
    with tempfile.TemporaryFile(mode='w+') as log:
        process = subprocess.Popen([sys.executable, '-I', '-m', 'inventor_kit.viewer',
            str(corpus/file), '--no-browser', *options], stdout=subprocess.PIPE, stderr=log, text=True)
        lines = queue.Queue()
        thread = threading.Thread(target=lambda: lines.put(process.stdout.readline()), daemon=True)
        thread.start()
        try:
            url = lines.get(timeout=15).strip()
            if not url.startswith('http://127.0.0.1:'):
                raise AssertionError('Installed viewer did not start')
            with urlopen(url, timeout=5) as response:
                assert b'Inventor Kit' in response.read()
            deadline = time.monotonic()+60
            while True:
                with urlopen(url+'state.json', timeout=5) as response:
                    scene = json.load(response)
                if scene['job_status'] in ('finished','failed'):
                    break
                if time.monotonic() >= deadline:
                    raise AssertionError('Installed viewer conversion timed out')
                time.sleep(.1)
            assert scene['stages']['tessellation'] in ('available', 'partial') and len(scene['nodes']) == occurrences
            assert sum(n['mesh_id'] is not None for n in scene['nodes']) == parts
            assert len(scene['omissions']) == omissions
            if scene['assembly'] is not None:
                assert scene['assembly']['displayed_instances'] == parts
                assert scene['assembly']['allow_unverified_state'] is True
            assert scene['complete'] is False and scene['thumbnails']
            for mesh in scene['meshes']:
                for buffer in mesh['buffers'].values():
                    with urlopen(url+buffer['resource'], timeout=5) as response:
                        assert len(response.read()) == buffer['bytes']
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
                try:
                    process.wait(5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            process.stdout.close()
            thread.join(1)
            log.seek(0)
            stderr = log.read()
        if process.returncode != 0:
            raise AssertionError(f'Installed viewer shutdown failed: {process.returncode}: {stderr}')
    print(json.dumps({'installed_viewer': 'passed', 'input': file, 'displayed_instances': parts,
                      'scope': 'Linux startup, assets, geometry and shutdown; not browser rendering'}))


def installed(corpus):
    import importlib.metadata
    import math
    import inventor_kit
    assert inventor_kit.capabilities()['support_level'] == 'verified_subset'
    assert not inventor_kit.capabilities()['part_geometry']['current_state_verified']
    assert inventor_kit.Limits().max_file_bytes == 128 * 1024 * 1024
    try:
        inventor_kit.inspect_file(corpus / 'SamplePart.ipt', limits=inventor_kit.Limits(max_file_bytes=1))
    except ValueError as error:
        assert 'file byte limit' in str(error)
    else:
        raise AssertionError('Installed extension ignored explicit limits')
    metadata = inventor_kit.inspect_file(corpus / 'SamplePart.ipt').metadata
    assert metadata.stages.geometry == 'not_attempted'
    assert len(metadata.find_properties(semantic_name='part_number')) == 3
    assert len(metadata.thumbnails) == 3
    assert all(p.state_binding == 'unresolved' for p in metadata.find_properties(semantic_name='part_number'))
    inventory = inventor_kit.inspect_file(corpus / 'SamplePart.ipt', include_candidates=True).geometry
    assert len(inventory.candidates) == 1 and inventory.selection.status == 'not_requested'
    assert 'cq_acis' not in sys.modules and 'cadquery' not in sys.modules
    saved = inventor_kit.inspect_assembly_file(corpus / 'm5-samplebg/Subassembly.iam')
    assembly = inventor_kit.read_assembly_file(corpus / 'm5-samplebg/Subassembly.iam')
    assert saved.stored_occurrences[0]['properties']
    assert 'cq_acis' not in sys.modules and 'cadquery' not in sys.modules
    import cq_acis
    from cq_acis import _native
    for module in (inventor_kit, inventor_kit._inventor, cq_acis, _native):
        if not Path(module.__file__).resolve().is_relative_to(Path(sys.prefix).resolve()):
            raise AssertionError(f'Imported checkout code: {module.__file__}')
    for name, volume in [('Cylinder.ipt', math.pi * 12.5**2 * 10), ('SamplePart.ipt', 3750)]:
        doc = inventor_kit.read_file(corpus / name)
        assert doc.summary['status'] == 'decoded_subset'
        assert doc.summary['model_analysis']['bodies']
        assert doc.geometry.selection.state_verification == 'unverified'
        explicit = inventor_kit.read_file(corpus / name, candidate_id=doc.geometry.candidates[0].id)
        assert explicit.model == doc.model
        strict = inventor_kit.read_file(corpus / name, require_current_state=True)
        assert strict.model is None and strict.geometry.selection.status == 'state_unverified'
        assert doc.model.to_native().to_model() == doc.model
        shape = doc.to_cadquery().val()
        assert shape.isValid() and len(shape.Solids()) == 1
        assert math.isclose(shape.Volume(), volume, rel_tol=1e-9)
    with tempfile.TemporaryDirectory(prefix='inventor-installed-step-') as temporary:
        report = assembly.to_cadquery(allow_unverified_state=True).export_step(Path(temporary) / 'assembly.step')
        assert report['roundtrip']['status'] == 'passed'
        assert report['roundtrip']['part_instances'] == 1
        assert report['current_state'] == 'unverified' and report['complete'] is False
        assert len(report['source_documents']) == 2
    import inventor_kit.assembly_step
    assert Path(inventor_kit.assembly_step.__file__).resolve().is_relative_to(Path(sys.prefix).resolve())
    print(json.dumps({'isolated_install': 'passed', 'python': sys.version,
                      'inventor-kit': importlib.metadata.version('inventor-kit'),
                      'cq-acis': importlib.metadata.version('cq-acis'),
                      'model_api': inventor_kit._inventor.MODEL_API_VERSION,
                      'document_api': inventor_kit._inventor.DOCUMENT_API_VERSION,
                      'geometry_inventory_api': inventor_kit._inventor.GEOMETRY_INVENTORY_API_VERSION,
                      'metadata_without_geometry_import': 'passed',
                      'assembly_step_roundtrip': 'passed'}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group()
    source.add_argument('--wheel', type=Path)
    source.add_argument('--sdist', type=Path)
    parser.add_argument('--cq-wheel', type=Path, help='Explicit prerelease cq-acis wheel; omit for PyPI-only dependency validation')
    parser.add_argument('--core-crate', type=Path, help='Explicit unpublished core .crate for provisional sdist validation')
    parser.add_argument('--bridge-crate', type=Path, help='Explicit unpublished .crate for provisional sdist validation')
    parser.add_argument('--python', default=sys.executable)
    parser.add_argument('--installed', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--viewer', action='store_true', help='Install the viewer extra and check its local server (Linux)')
    parser.add_argument('--browser', action='store_true', help='Also run the viewer Playwright suite against the installed package (requires --viewer)')
    parser.add_argument('--corpus', type=Path, default=ROOT / 'fixtures/public')
    args = parser.parse_args()
    if args.browser and not args.viewer:
        parser.error('--browser requires --viewer')
    if args.viewer and os.name == 'nt':
        parser.error('Viewer distribution smoke currently targets Linux; Windows qualification is separate')
    if args.installed:
        installed(args.corpus)
        if args.viewer:
            installed_viewer(args.corpus)
            installed_viewer(args.corpus, 'm5-samplebg/Subassembly.iam', ('--allow-unverified-state',))
            installed_viewer(args.corpus, 'm5-samplebg/SampleBg.iam',
                ('--allow-unverified-state', '--allow-partial', '--search-root', str(args.corpus/'m5-samplebg/iPartSample')),
                parts=5, occurrences=7, omissions=2)
        return
    if args.wheel is None and args.sdist is None:
        parser.error('--wheel or --sdist is required')
    if (args.bridge_crate or args.core_crate) and not args.sdist:
        parser.error('--bridge-crate/--core-crate apply only to --sdist')
    environment = dict(os.environ)
    for name in ('PYTHONPATH', 'PYTHONHOME', 'VIRTUAL_ENV', 'CARGO_TARGET_DIR'):
        environment.pop(name, None)
    with tempfile.TemporaryDirectory(prefix='inventor-independent-') as temporary:
        root = Path(temporary)
        wheel = args.wheel.resolve() if args.wheel else None
        if args.sdist:
            import tomllib
            with tarfile.open(args.sdist) as archive:
                archive.extractall(root / 'source', filter='data')
            projects = list((root/'source').glob('*/pyproject.toml'))
            if len(projects) != 1:
                raise ValueError('Expected one source project')
            project = projects[0].parent
            patches = []
            for name, crate in [('acis-py-bridge', args.bridge_crate), ('acis-core', args.core_crate)]:
                if crate is None:
                    continue
                with tarfile.open(crate) as archive:
                    archive.extractall(root/'staged'/name, filter='data')
                manifests = list((root/'staged'/name).glob('*/Cargo.toml'))
                if len(manifests) != 1:
                    raise ValueError(f'Expected one staged {name} crate')
                package = tomllib.loads(manifests[0].read_text())['package']
                if (package['name'], package['version']) != (name, '0.3.2'):
                    raise ValueError(f'Unexpected staged {name}')
                patches.append(name + ' = { path = '+json.dumps(str(manifests[0].parent))+' }\n')
            if patches:
                config = project/'.cargo/config.toml'
                config.parent.mkdir(exist_ok=True)
                config.write_text('[patch.crates-io]\n'+''.join(patches))
            environment['CARGO_TARGET_DIR'] = str(root/'target')
            subprocess.run([sys.executable, '-m', 'maturin', 'build', '--release', '--locked', '--offline', '--out', str(root/'wheels'), '--interpreter', sys.executable], cwd=project, env=environment, check=True)
            wheels = list((root/'wheels').glob('*.whl'))
            if len(wheels) != 1:
                raise ValueError('Expected one rebuilt wheel')
            wheel = wheels[0]
        subprocess.run([args.python, '-m', 'venv', str(root/'venv')], env=environment, check=True)
        python = root/'venv'/('Scripts/python.exe' if os.name == 'nt' else 'bin/python')
        packages = [str(wheel) + ('[viewer]' if args.viewer else '')] + ([str(args.cq_wheel.resolve())] if args.cq_wheel else [])
        subprocess.run([str(python), '-m', 'pip', 'install', '--quiet', '--disable-pip-version-check', '--only-binary=:all:', *packages], env=environment, check=True)
        subprocess.run([str(python), '-m', 'pip', 'check'], env=environment, check=True)
        subprocess.run([str(python), '-I', '-X', 'faulthandler', '-u', str(Path(__file__).resolve()), '--installed', '--corpus', str(args.corpus.resolve()), *(['--viewer'] if args.viewer else [])], cwd=root, env=environment, check=True)
        if args.browser:
            browser_environment = dict(environment, VIEWER_PYTHON=str(python), VIEWER_CWD=str(root), VIEWER_CORPUS=str(args.corpus.resolve()))
            subprocess.run(['npm', 'run', 'test', '--prefix', str(ROOT/'viewer')], cwd=root, env=browser_environment, check=True)
        print(json.dumps({'dependency_mode': 'local cq-acis release candidate' if args.cq_wheel else 'PyPI',
                          'interpreter_shutdown': 'passed',
                          'bridge_mode': 'staged unpublished crate' if args.bridge_crate else 'registry or prebuilt wheel',
                          'core_mode': 'staged unpublished crate' if args.core_crate else 'registry or prebuilt wheel'}))


if __name__ == '__main__':
    main()
