"""Pinned native assembly regression and frozen-profile holdout; no vendor oracle claim."""
import argparse
import hashlib
import json
from pathlib import Path
import tempfile

import inventor_kit as ik

ROOT = Path(__file__).resolve().parents[1]


def metrics(shape):
    box = shape.BoundingBox()
    return dict(valid=shape.isValid(), solids=len(shape.Solids()), volume_mm3=shape.Volume(),
                bbox_mm=[getattr(box, k) for k in ('xmin', 'ymin', 'zmin', 'xmax', 'ymax', 'zmax')])


def validate(fixtures):
    manifest = json.loads((ROOT / 'fixtures/assembly-manifest.json').read_text())
    for row in manifest:
        data = (fixtures / row['file']).read_bytes()
        if len(data) != row['bytes'] or hashlib.sha256(data).hexdigest() != row['sha256']:
            raise ValueError(f'Fixture mismatch: {row["file"]}')
    cases = []
    expected = {'Subassembly.iam': (1, 'resolved', 1), 'SampleBg.iam': (7, 'partial', 5),
                'BoltedConnection.iam': (1, 'partial', 0)}
    for name, (count, structure, converted) in expected.items():
        graph = ik.read_assembly_file(fixtures / 'm5-samplebg' / name,
                                      search_roots=[fixtures / 'm5-samplebg/iPartSample'])
        result = graph.to_cadquery(allow_partial=True, allow_unverified_state=True)
        if len(graph.instances) != count or graph.structure_status != structure or result.converted_instances != converted:
            raise AssertionError(f'Assembly regression: {name}')
        case = dict(file=name, source_sha256=graph.definitions[graph.summary['root']].document.summary['source_sha256'],
                    structure_status=graph.structure_status, current_state=graph.summary['current_state'],
                    definitions=len(graph.definitions), occurrences=len(graph.instances),
                    instances=[dict(path=i.path, definition=i.definition, resolution=i.resolution,
                                    local_transform_mm=i.local_transform_mm, world_transform_mm=i.world_transform_mm,
                                    suppressed=i.suppressed, visible=i.visible, substitute=i.substitute) for i in graph.instances],
                    converted_instances=result.converted_instances, converted_definitions=result.converted_definitions,
                    omissions=[vars(o) for o in result.omissions], reference_issues=result.reference_issues,
                    diagnostics=result.diagnostics, complete=result.complete,
                    geometry=metrics(result.assembly.toCompound()) if converted else None)
        if case['geometry'] and (not case['geometry']['valid'] or case['geometry']['solids'] != converted):
            raise AssertionError(f'Invalid assembly geometry: {name}')
        if converted:
            with tempfile.TemporaryDirectory(prefix='inventor-assembly-step-') as temporary:
                step = result.export_step(Path(temporary) / (name + '.step'), allow_partial=True)
            case['step_roundtrip'] = {k: step[k] for k in
                ('step_sha256', 'roundtrip', 'expected', 'actual', 'color_origin', 'name_origin', 'complete')}
        else:
            case['step_roundtrip'] = dict(status='not_exported', reason='no converted parts')
        cases.append(case)
    holdout = ik.inspect_assembly_file(fixtures / 'm5-holdout/wing_assembly.iam')
    if holdout.status != 'unavailable' or holdout.occurrences:
        raise AssertionError('Frozen unsupported-profile holdout changed; review support before changing this gate')
    return dict(schema_version=1, fixture_manifest_sha256=hashlib.sha256((ROOT / 'fixtures/assembly-manifest.json').read_bytes()).hexdigest(),
                dependency_versions=dict(acis_core='0.3.3', acis_py_bridge='0.3.3', model_api=2, assembly_api=1),
                fixtures=len(manifest), native_cases=cases,
                holdout=dict(file='wing_assembly.iam', source_sha256=holdout.summary['source_sha256'],
                             schema=holdout.summary['ufrx']['schema'], sections=holdout.summary['ufrx']['sections'],
                             status=holdout.status, diagnostics=holdout.diagnostics, used_for_tuning=False),
                regression_passed=True,
                vendor_comparison=dict(status='not_collected', multilevel_packaged_assembly=False,
                                       current_state=False, world_transform=False, bbox=False),
                current_state_verified=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'fixtures/public')
    parser.add_argument('--output', type=Path, default=ROOT / 'internal/reports/latest/assembly_validation.json')
    args = parser.parse_args()
    report = validate(args.fixtures)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
    print(json.dumps(dict(report=str(args.output), regression_passed=True, current_state_verified=False)))


if __name__ == '__main__':
    main()
