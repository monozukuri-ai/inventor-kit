"""Rebuild/install distributions outside checkouts; local release candidates are explicit."""
from pathlib import Path
import argparse
import hashlib
import json
import platform
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
    with tempfile.TemporaryDirectory(prefix='inventor-viewer-smoke-') as sessions, tempfile.TemporaryFile(mode='w+') as log:
        process = subprocess.Popen([sys.executable, '-I', '-X', 'faulthandler', '-m', 'inventor_kit.viewer',
            str(corpus/file), '--no-browser', *options], stdout=subprocess.PIPE, stderr=log, text=True,
            env=dict(os.environ, TMPDIR=sessions, TEMP=sessions, TMP=sessions),
            creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == 'nt' else 0)
        lines = queue.Queue()
        thread = threading.Thread(target=lambda: lines.put(process.stdout.readline()), daemon=True)
        thread.start()
        failure = None
        try:
            try:
                url = lines.get(timeout=15).strip()
            except queue.Empty as error:
                raise AssertionError('Installed viewer startup timed out before printing its URL') from error
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
            assert len(scene['part']['omissions'] if scene.get('part') else scene['omissions']) == omissions
            if scene['assembly'] is not None:
                assert scene['assembly']['displayed_instances'] == parts
                assert scene['assembly']['allow_unverified_state'] is True
            assert scene['complete'] is False and scene['thumbnails']
            resources = 0
            for mesh in scene['meshes']:
                for buffer in mesh['buffers'].values():
                    with urlopen(url+buffer['resource'], timeout=5) as response:
                        assert len(response.read()) == buffer['bytes']
                    resources += 1
        except Exception as error:
            failure = error
        finally:
            if process.poll() is None:
                try:
                    # Windows console events require a distinct process group;
                    # terminate()/SIGTERM would bypass the viewer's cleanup.
                    process.send_signal(signal.CTRL_BREAK_EVENT if os.name == 'nt' else signal.SIGINT)
                    process.wait(10)
                except (OSError, subprocess.TimeoutExpired) as error:
                    failure = failure or error
                finally:
                    if process.poll() is None:
                        process.kill()
                        process.wait()
            process.stdout.close()
            thread.join(1)
            log.seek(0)
            stderr = log.read()
        if failure is not None:
            raise AssertionError(f'{failure}; viewer exit={process.returncode}; stderr:\n{stderr}') from failure
        if process.returncode != 0:
            raise AssertionError(f'Installed viewer shutdown failed: {process.returncode}: {stderr}')
        if list(Path(sessions).iterdir()):
            raise AssertionError('Installed viewer left temporary session data after shutdown')
    result = {'displayed_instances': parts, 'occurrences': occurrences, 'omissions': omissions,
              'mesh_buffers_fetched': resources, 'shutdown': 'passed'}
    print(json.dumps({'installed_viewer': 'passed', **result}))
    return result


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
    # Exercise public cq-acis 0.3.8 with the registry core/bridge after installation.
    name = 'INV_nist_ftc_07_asme1_2021.ipt'
    item = next(e for e in json.loads((ROOT/'fixtures/manifest.json').read_text()) if e['file'] == name)
    data = (corpus/name).read_bytes()
    assert len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256']
    doc = inventor_kit.read(data, source_id=name)
    converter = cq_acis.CadQueryConverter(doc.model)
    placement = converter._body_placement(doc.model.bodies()[0])
    for index in (1108, 1626, 1412):  # Two direct finite surfaces and a subtype reference.
        surface = converter._resolve_geometry(cq_acis.EntityRef(index))
        assert isinstance(surface, cq_acis.BSplineSurfaceEntity)
        surface.validate()
        assert surface.u_range.lower is not None and surface.v_range.upper is not None
    pcurve = doc.model.to_native().linear_surface_pcurve(cq_acis.EntityRef(486), cq_acis.EntityRef(226))
    assert pcurve is not None and pcurve.support.entity_index == 226
    assert pcurve.raw == doc.model.resolve(cq_acis.EntityRef(486))
    coedge = converter._require(264, cq_acis.CoedgeEntity, context='installed tolerant boundary')
    edge = converter._edge(coedge, placement)
    assert edge.isValid() and math.isclose(edge.Length(), 0.7447668984110175, abs_tol=1e-9)
    assert hasattr(converter, 'tolerant_vertex_envelopes')
    for index in (1615, 3616):
        use = converter._require(index, cq_acis.CoedgeEntity, context='installed inline boundary')
        assert converter._edge(use, placement).isValid()
    assert converter._face(doc.model.resolve(2782), placement).isValid()
    assert {e['coedge'] for e in converter.inline_reparameterizations} == {1615,3616}
    view = doc.model.to_native().supported_curve(4022)
    assert view.curve.raw == doc.model.resolve(4022)
    complete = doc.to_cadquery().val()
    assert complete.isValid() and len(complete.Solids()) == 1 and len(complete.Faces()) == 258
    from OCP.BRepGProp import BRepGProp
    from OCP.GProp import GProp_GProps
    adaptive = []
    for epsilon in (1e-8, 1e-10, 1e-12):
        properties = GProp_GProps()
        error = BRepGProp.VolumeProperties_s(complete.wrapped, properties, epsilon)
        adaptive.append(dict(epsilon=epsilon, volume_mm3=properties.Mass(), estimated_error=error))
    volume = complete.Volume()
    expected_volume = 1678794.5921294673
    box = complete.BoundingBox()
    comparison = dict(file=name, sha256=item['sha256'], volume_mm3=volume,
        expected_volume_mm3=expected_volume, absolute_error_mm3=abs(volume-expected_volume),
        relative_error=abs(volume-expected_volume)/expected_volume, adaptive=adaptive,
        faces=len(complete.Faces()), solids=len(complete.Solids()), area_mm2=complete.Area(),
        bbox_mm=[getattr(box, k) for k in ('xmin','ymin','zmin','xmax','ymax','zmax')],
        platform=platform.system(), machine=platform.machine(),
        packages={p: importlib.metadata.version(p) for p in ('inventor-kit','cq-acis','cadquery','cadquery-ocp')})
    print(json.dumps({'ftc07_geometry': comparison}, allow_nan=False), flush=True)
    # The default (non-adaptive) integral differs by 3.609e-4 mm3 on
    # macOS arm64. Use a converged adaptive integral, preserving the 1e-10
    # relative acceptance bound. This reference is an OCCT regression value,
    # not a vendor oracle; Linux and arm64 agree within 1.066e-4 mm3.
    adaptive_reference = 1678794.951282398
    fine, medium = adaptive[-1], adaptive[-2]
    assert 0 <= fine['estimated_error'] <= 1e-10, comparison
    assert math.isclose(fine['volume_mm3'], medium['volume_mm3'], rel_tol=1e-10), comparison
    assert math.isclose(fine['volume_mm3'], adaptive_reference, rel_tol=1e-10), comparison
    for index in (332, 1164, 2336, 4351):
        assert converter._face(doc.model.resolve(index), placement).isValid()
    assert {e['vertex'] for e in converter.tolerant_vertex_envelopes} == {3618}
    assert {e['coedge'] for e in converter.saved_pcurve_reparameterizations} == {2146, 2130, 3613}

    # Exercise the public body API and installed CLI, including partial consent.
    mixed = inventor_kit.read_file(corpus/'INV_nist_ftc_06_asme1_2021.ipt').convert_bodies()
    assert len(mixed.bodies) == 3 and mixed.bodies[0].metrics['faces'] == 146
    assert not mixed.geometry_complete and not mixed.current_state_verified
    with tempfile.TemporaryDirectory(prefix='inventor-body-smoke-') as temporary:
        output = Path(temporary)/'body.step'
        try:
            mixed.export_step(output)
        except inventor_kit.BodyConversionError:
            pass
        else:
            raise AssertionError('Installed body API silently discarded unsupported bodies')
        report = mixed.export_step(output, body_ids=[mixed.bodies[0].id], allow_partial=True)
        assert report['roundtrip']['status'] == 'passed' and len(report['omissions']) == 2
        assert report['selection_complete'] and not report['geometry_complete']
        cli = subprocess.run([sys.executable, '-I', '-m', 'inventor_kit', str(corpus/'SamplePart.ipt'),
            '--step', str(Path(temporary)/'cli.step'), '--jsonl'], capture_output=True, text=True, timeout=90)
        assert cli.returncode == 0, (cli.returncode, cli.stdout, cli.stderr)
        assert json.loads(cli.stdout)['export']['roundtrip']['status'] == 'passed'

    # Keep the FTC06 auxiliary open bodies visible in the conversion contract.
    for year, seam_faces in ((2021, {247, 332, 405, 464}), (2024, {233, 471, 634, 676})):
        doc = inventor_kit.read_file(corpus / f'INV_nist_ftc_06_asme1_{year}.ipt')
        assert [body.index for body in doc.model.bodies()] == [1, 2, 3]
        converter = cq_acis.CadQueryConverter(doc.model)
        solid = converter.convert_body(doc.model.bodies()[0])
        assert solid.isValid() and len(solid.Solids()) == 1 and len(solid.Faces()) == 146
        assert all(shell.Closed() for shell in solid.Shells())
        assert math.isclose(solid.Volume(), 3326997.715700659, rel_tol=1e-8)
        assert {e['face'] for e in converter.periodic_seam_faces} == seam_faces
        assert all(e['max_deviation_mm'] <= e['tolerance_mm'] for e in converter.periodic_seam_faces)
        try:
            doc.to_cadquery()
        except cq_acis.CadQueryConversionError as error:
            assert error.code == 'geometry.sewing_no_shell'
        else:
            raise AssertionError('FTC06 complete conversion omitted auxiliary open bodies')
    doc = inventor_kit.read_file(corpus / 'INV_nist_ctc_04_asme1_2021.ipt')
    solid = doc.to_cadquery().val()
    assert solid.isValid() and len(solid.Solids()) == 1 and len(solid.Faces()) == 368
    assert all(shell.Closed() for shell in solid.Shells())
    assert math.isclose(solid.Volume(), 17526545.186092895, rel_tol=1e-8)

    with tempfile.TemporaryDirectory(prefix='inventor-installed-step-') as temporary:
        report = assembly.to_cadquery(allow_unverified_state=True).export_step(Path(temporary) / 'assembly.step')
        assert report['roundtrip']['status'] == 'passed'
        assert report['roundtrip']['part_instances'] == 1
        assert report['current_state'] == 'unverified' and report['complete'] is False
        assert len(report['source_documents']) == 2
    import inventor_kit.assembly_step
    assert Path(inventor_kit.assembly_step.__file__).resolve().is_relative_to(Path(sys.prefix).resolve())
    # Keep a real reader open during atomic publication. On Windows this
    # exercises the sharing violation seen between HTTP reads and Job.poll.
    import threading
    from inventor_kit.viewer.scene import empty_scene, write_scene
    with tempfile.TemporaryDirectory(prefix='inventor-snapshot-smoke-') as temporary:
        directory = Path(temporary)
        write_scene(directory, empty_scene('previous'))
        with (directory/'state.json').open('rb') as reader:
            assert json.load(reader)['source']['name'] == 'previous'
            release = threading.Timer(0.2, reader.close)
            release.start()
            try:
                write_scene(directory, empty_scene('replacement'))
            finally:
                release.join()
        assert json.loads((directory/'state.json').read_text())['source']['name'] == 'replacement'
    print(json.dumps({'isolated_install': 'passed', 'python': sys.version,
                      'inventor-kit': importlib.metadata.version('inventor-kit'),
                      'cq-acis': importlib.metadata.version('cq-acis'),
                      'model_api': inventor_kit._inventor.MODEL_API_VERSION,
                      'document_api': inventor_kit._inventor.DOCUMENT_API_VERSION,
                      'geometry_inventory_api': inventor_kit._inventor.GEOMETRY_INVENTORY_API_VERSION,
                      'metadata_without_geometry_import': 'passed',
                      'tolerant_trim_components': 'passed',
                      'analytic_closure': 'passed',
                      'assembly_step_roundtrip': 'passed',
                      'viewer_snapshot_publication': 'passed'}))


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
    parser.add_argument('--viewer', action='store_true', help='Install the viewer extra and check its local server and shutdown')
    parser.add_argument('--browser', action='store_true', help='Also run the viewer Playwright suite against the installed package (requires --viewer)')
    parser.add_argument('--report', type=Path, help='Write a selected public qualification summary (requires --viewer)')
    parser.add_argument('--corpus', type=Path, default=ROOT / 'fixtures/public')
    args = parser.parse_args()
    if args.browser and not args.viewer:
        parser.error('--browser requires --viewer')
    if args.browser and sys.platform != 'linux':
        parser.error('--browser is qualified on Linux Chromium only')
    if args.report and not args.viewer:
        parser.error('--report requires --viewer')
    if args.report:
        args.report = args.report.resolve()
        args.report.unlink(missing_ok=True)  # Never leave a stale pass after a failed retry.
    if args.installed:
        installed(args.corpus)
        if args.viewer:
            cases = {
                'part': installed_viewer(args.corpus),
                'assembly': installed_viewer(args.corpus, 'm5-samplebg/Subassembly.iam', ('--allow-unverified-state',)),
                'partial_assembly': installed_viewer(args.corpus, 'm5-samplebg/SampleBg.iam',
                    ('--allow-unverified-state', '--allow-partial', '--search-root', str(args.corpus/'m5-samplebg/iPartSample')),
                    parts=5, occurrences=7, omissions=2),
                'partial_part': installed_viewer(args.corpus, 'INV_nist_ftc_06_asme1_2021.ipt',
                    ('--allow-partial',), parts=1, occurrences=3, omissions=2),
            }
            if args.report:
                from importlib.metadata import version
                machine = platform.machine().lower()
                architecture = {'amd64': 'x86_64', 'aarch64': 'arm64'}.get(machine, machine)
                operating_system = {'Darwin': 'macos'}.get(platform.system(), platform.system().lower())
                args.report.write_text(json.dumps({
                    'platform': operating_system + '-' + architecture,
                    'python': platform.python_version(),
                    'dependencies': {name: version(name) for name in
                        ('inventor-kit', 'cq-acis', 'cadquery', 'cadquery-ocp', 'ocp-tessellate')},
                    'cases': cases,
                }), encoding='utf-8')
        return
    if args.wheel is None and args.sdist is None:
        parser.error('--wheel or --sdist is required')
    if (args.bridge_crate or args.core_crate) and not args.sdist:
        parser.error('--bridge-crate/--core-crate apply only to --sdist')
    environment = dict(os.environ)
    for name in ('PYTHONPATH', 'PYTHONHOME', 'VIRTUAL_ENV', 'CARGO_TARGET_DIR'):
        environment.pop(name, None)
    artifact = (args.wheel or args.sdist).resolve()
    artifact_hash = hashlib.sha256(artifact.read_bytes()).hexdigest()
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
                if (package['name'], package['version']) != (name, '0.3.8'):
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
            from check_distribution import check
            check(wheel)  # Inspect the rebuilt archive as well as the source archive.
        subprocess.run([args.python, '-m', 'venv', str(root/'venv')], env=environment, check=True)
        python = root/'venv'/('Scripts/python.exe' if os.name == 'nt' else 'bin/python')
        packages = [str(wheel) + ('[viewer]' if args.viewer else '')] + ([str(args.cq_wheel.resolve())] if args.cq_wheel else [])
        subprocess.run([str(python), '-m', 'pip', 'install', '--quiet', '--disable-pip-version-check', '--only-binary=:all:', *packages], env=environment, check=True)
        subprocess.run([str(python), '-m', 'pip', 'check'], env=environment, check=True)
        subprocess.run([str(python), '-I', '-X', 'faulthandler', '-u', str(Path(__file__).resolve()), '--installed', '--corpus', str(args.corpus.resolve()), *(['--viewer', '--report', str(root/'installed.json')] if args.viewer else [])], cwd=root, env=environment, check=True)
        if args.viewer:
            details = json.loads((root/'installed.json').read_text(encoding='utf-8'))
        if args.browser:
            browser_environment = dict(environment, VIEWER_PYTHON=str(python), VIEWER_CWD=str(root), VIEWER_CORPUS=str(args.corpus.resolve()))
            subprocess.run(['npm', 'run', 'test', '--prefix', str(ROOT/'viewer')], cwd=root, env=browser_environment, check=True)
        print(json.dumps({'dependency_mode': 'local cq-acis release candidate' if args.cq_wheel else 'PyPI',
                          'interpreter_shutdown': 'passed',
                          'bridge_mode': 'staged unpublished crate' if args.bridge_crate else 'registry or prebuilt wheel',
                          'core_mode': 'staged unpublished crate' if args.core_crate else 'registry or prebuilt wheel'}))

    # The interpreter and temporary environment must both have shut down first.
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps({
            'schema_version': 1, 'status': 'passed',
            'artifact': {'file': artifact.name, 'sha256': artifact_hash},
            **details,
            'dependency_mode': 'local' if (args.cq_wheel or args.core_crate or args.bridge_crate) else 'published',
            'pip_check': 'passed', 'interpreter_shutdown': 'passed',
            'browser': 'linux-chromium-passed' if args.browser else 'not_run',
            'current_state_verified': False,
        }, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
