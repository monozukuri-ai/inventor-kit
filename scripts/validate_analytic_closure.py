"""Pinned FTC06 cylinder seams and CTC04 shell closure, with explicit body scope.

Requires cq-acis 0.3.7 or later with periodic_seam_faces and shell_closure_checks diagnostics.
FTC06 has two auxiliary open planar bodies: complete-solid conversion must
still fail rather than silently omit them. No holdout is used by this gate.
"""
import argparse
from dataclasses import asdict
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path

import cadquery as cq
import cq_acis as acis
import inventor_kit
from corpus_manifest import load_manifest
from oracle_contract import shape_metrics
from inventor_kit.viewer.scene import Options, build_scene, write_scene
from inventor_kit.viewer.tessellation import MeshBuilder

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    'INV_nist_ftc_06_asme1_2021.ipt': (148, 146, {247, 332, 405, 464}),
    'INV_nist_ftc_06_asme1_2024.ipt': (148, 146, {233, 471, 634, 676}),
    'INV_nist_ctc_04_asme1_2021.ipt': (368, 368, set()),
}


def close_metrics(before, after):
    assert before['faces'] == after['faces'] and before['solid_count'] == after['solid_count']
    for key in ('volume_mm3', 'area_mm2'):
        assert math.isclose(before[key], after[key], rel_tol=1e-8, abs_tol=1e-6), key
    assert all(abs(a-b) <= 1e-6 for a, b in zip(before['bbox_mm'], after['bbox_mm']))


def validate(item, directory):
    name = item['file']
    expected_faces, primary_faces, seam_faces = TARGETS[name]
    path = ROOT/'fixtures/public'/name
    data = path.read_bytes()
    assert len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256']
    doc = inventor_kit.read(data, source_id=name)
    converter = acis.CadQueryConverter(doc.model)
    assert hasattr(converter, 'periodic_seam_faces') and hasattr(converter, 'shell_closure_checks'), (
        'cq-acis with qualified cylinder seams and shell-closure checks is required')
    model = converter.model
    bodies = model.bodies()
    assert [body.index for body in bodies] == ([1, 2, 3] if seam_faces else [1])
    placements = {b.index: converter._body_placement(b) for b in bodies}
    face_results = []
    by_body = {b.index: [] for b in bodies}
    directory.mkdir(parents=True, exist_ok=True)
    for face in (e for e in model.entities if isinstance(e, acis.FaceEntity)):
        shell = model.resolve(face.shell)
        lump = model.resolve(shell.lump)
        shape = converter._face(face, placements[lump.body.index])
        assert shape.isValid() and shape.Area() > 0
        span = face.raw.source
        offset = doc.summary['carrier']['kernel_offset']
        assert face.raw.raw_data == doc.kernel_bytes[span.start_offset-offset:span.end_offset-offset]
        face_results.append(dict(face=face.index, body=lump.body.index, source=asdict(span),
            raw_sha256=hashlib.sha256(face.raw.raw_data).hexdigest(), area_mm2=shape.Area()))
        by_body[lump.body.index].append(shape)
    assert len(face_results) == expected_faces
    assert {entry['face'] for entry in converter.periodic_seam_faces} == seam_faces
    assert all(e['max_deviation_mm'] <= e['tolerance_mm'] for e in converter.periodic_seam_faces)
    assert model.to_native().to_model() == model

    mesh_builder = MeshBuilder(directory, Options())
    body_results = []
    primary_metrics = None
    for body in bodies:
        body_converter = acis.CadQueryConverter(model)
        entry = dict(body=body.index, source_face_count=len(by_body[body.index]))
        try:
            result = body_converter.convert_body(body)
        except acis.CadQueryConversionError as error:
            assert seam_faces and body.index in (2, 3) and error.code == 'geometry.sewing_no_shell'
            assert len(by_body[body.index]) == 1
            result = by_body[body.index][0]
            entry.update(status='open_auxiliary_face', conversion_error=error.code,
                reason=str(error), surface_types=[model.resolve(model.resolve(i).surface).raw.type_name
                    for i in (11 if body.index == 2 else 12,)],
                preview_scope='individual source face only; not a converted solid body')
        else:
            assert body.index == 1 and result.isValid() and len(result.Solids()) == 1
            assert len(result.Faces()) == primary_faces
            assert all(shell.Closed() for shell in result.Shells())
            primary_metrics = shape_metrics((result,))
            assert primary_metrics['volume_mm3'] > 0
            step = directory/'primary-body.step'
            cq.exporters.export(result, str(step))
            reimported = cq.importers.importStep(str(step)).vals()
            assert all(s.isValid() for s in reimported)
            close_metrics(primary_metrics, shape_metrics(reimported))
            entry.update(status='converted_valid_solid', metrics=primary_metrics,
                closure_checks=body_converter.shell_closure_checks,
                step_roundtrip='passed', step_file=str(step),
                step_sha256=hashlib.sha256(step.read_bytes()).hexdigest())
        mesh = mesh_builder.add(result, f'body-{body.index}')
        assert mesh['face_count'] == entry['source_face_count']
        entry['mesh'] = mesh
        body_results.append(entry)

    try:
        shapes = doc.to_cadquery().vals()
    except acis.CadQueryConversionError as error:
        assert seam_faces and error.code == 'geometry.sewing_no_shell'
        whole = dict(status='rejected', error_code=error.code, reason=str(error),
            explanation='two auxiliary open planar bodies are retained; never omitted from whole-part conversion')
    else:
        assert not seam_faces and len(shapes) == 1
        close_metrics(primary_metrics, shape_metrics(shapes))
        whole = dict(status='converted_valid_solid', metrics=primary_metrics)

    viewer_dir = directory/'viewer'
    viewer_dir.mkdir(exist_ok=True)
    scene = build_scene(path, viewer_dir, Options())
    write_scene(viewer_dir, scene)
    assert scene['current_state'] == 'unverified' and not scene['complete']
    if seam_faces:
        assert scene['stages']['conversion'] == 'failed' and not scene['meshes']
        assert any(e['code'] == 'geometry.sewing_no_shell' for e in scene['diagnostics'])
    else:
        assert scene['stages']['conversion'] == scene['stages']['tessellation'] == 'available'
        assert sum(m['face_count'] for m in scene['meshes']) == expected_faces
        assert [node['body_index'] for node in scene['nodes']] == [1]
    return dict(file=name, sha256=item['sha256'], split=item['split'],
        candidate_id=doc.geometry.selection.selected_id, faces=face_results,
        seam_checks=converter.periodic_seam_faces, bodies=body_results,
        tessellation=dict(mesh_builder.settings(), cone_apex_normal_count=mesh_builder.cone_apex_normal_count),
        whole_part=whole,
        viewer=dict(stages=scene['stages'], meshes=len(scene['meshes']),
            current_state=scene['current_state'], complete=scene['complete']))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'internal/reports/latest/analytic-closure.json')
    args = parser.parse_args()
    items = {e['file']: e for e in load_manifest(split='regression')}
    results = []
    for name in TARGETS:
        entry = validate(items[name], args.output.parent/'analytic-closure-artifacts'/Path(name).stem)
        results.append(entry)
        print(name, len(entry['faces']), entry['whole_part']['status'], flush=True)
    first, second = (entry['bodies'][0]['metrics'] for entry in results[:2])
    close_metrics(first, second)
    report = dict(scope='Local saved B-rep checks; no vendor/current-state or platform qualification',
        packages={p: importlib.metadata.version(p) for p in ('inventor-kit', 'cq-acis', 'cadquery', 'cadquery-ocp')},
        module_files=dict(cq_acis=acis.__file__, inventor_kit=inventor_kit.__file__,
                          acis_native=str(Path(acis._native.__file__).resolve())),
        native_sha256=hashlib.sha256(Path(acis._native.__file__).read_bytes()).hexdigest(),
        core_version=acis._native.CORE_VERSION, results=results, ftc06_cross_version_metrics='matched',
        holdouts_used=False)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, allow_nan=False)+'\n')


if __name__ == '__main__':
    main()
