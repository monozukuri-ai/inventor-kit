"""Exercise an installed writer with synthetic geometry; no private CAD inputs."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def installed_writer():
    import cadquery as cq
    import inventor_kit as ik
    from inventor_kit.assembly_step import _write_xde
    from inventor_kit.step_parts import _compare
    for module in (ik, ik._inventor):
        if not Path(module.__file__).resolve().is_relative_to(Path(sys.prefix).resolve()):
            raise AssertionError('Writer smoke imported source checkout code')
    assert ik.capabilities()['part_writer']['available']
    with tempfile.TemporaryDirectory(prefix='inventor-installed-write-') as temporary:
        root = Path(temporary)
        body = cq.Workplane('XY').box(20, 12, 4, centered=False).val()
        body = body.cut(cq.Workplane('XY').center(6, 6).circle(2).extrude(4).val()).Solids()[0]
        from cq_acis.authoring import from_cadquery
        from cq_acis import _native
        authored = from_cadquery(body)
        restored = _native.author_analytic(json.loads(json.dumps(authored.to_spec())))
        assert restored.to_sab(history=False) == authored.to_sab(history=False)
        result = ik.new_part(name='単独部品', title='Writer smoke', part_number='W-001').add_body(
            body, name='本体', feature_name='基本形状').saveas(root/'部品.ipt')
        doc = ik.read_file(result.path)
        assert _compare(doc.to_cadquery().val(), body)['status'] == 'passed'
        properties = {p.semantic_name:p.value for s in doc.metadata.property_sets for p in s.properties if p.semantic_name}
        assert properties == dict(title='Writer smoke', part_number='W-001')
        assert result.native_validation == 'not_run'
        assert ik.capabilities()['assembly_writer']['available']
        fixed = ik.new_assembly(name='Installed assembly', part_number='ASM-001')
        fixed.add_part(result.path, name='Part:1').add_part(result.path, name='Part:2')
        bundle = fixed.save_bundle(root/'iam-api', filename='組立.iam')
        assert bundle.definition_count == 1 and bundle.occurrence_count == 2
        assert bundle.native_validation == 'not_run'
        moved = root/'moved bundle'
        bundle.directory.rename(moved)
        resolved = ik.read_assembly_file(moved/'組立.iam')
        assert resolved.structure_status == 'resolved' and len(resolved.instances) == 2
        assert resolved.to_cadquery(allow_unverified_state=True).converted_instances == 2
        process = subprocess.run([sys.executable, '-I', '-m', 'inventor_kit', 'write-assembly',
            str(root/'iam-cli'), str(result.path), '--timeout', '120'], capture_output=True,
            text=True, encoding='utf-8', timeout=150, check=True)
        assert json.loads(process.stdout)['published']
        assembly = cq.Assembly(name='Root')
        placed_body = cq.Workplane('XY').box(20, 12, 4, centered=False).val()
        group = cq.Assembly(name='Group', loc=cq.Location((71,-12,3), (0,0,1), 31))
        locations = [cq.Location((8,0,2), (0,1,0), 17), cq.Location((-9,4,1))]
        # Repeated definition and noncommuting transforms catch double placement.
        for index, location in enumerate(locations):
            group.add(placed_body, name=f'Part{index}', loc=location)
        assembly.add(group)
        source=root/'source.step'
        _write_xde(assembly, source)
        for mode in ('api', 'cli'):
            output=root/mode
            if mode == 'api':
                assert ik.write_step_parts(source, output).part_count == 2
            else:
                process = subprocess.run([sys.executable, '-I', '-m', 'inventor_kit', 'write-step',
                    str(source), str(output), '--timeout', '120'], capture_output=True, text=True,
                    encoding='utf-8', timeout=150, check=True)
                result=json.loads(process.stdout)
                assert result['status']=='success' and result['published'] and result['part_count']==2
            batch=json.loads((output/'batch.json').read_text(encoding='utf-8'))
            assert len(batch['parts'])==2 and batch['native_validation']=='not_run'
            for row, location in zip(batch['parts'], locations, strict=True):
                artifact=output/row['artifact']['path']
                assert hashlib.sha256(artifact.read_bytes()).hexdigest()==row['artifact']['sha256']
                assert _compare(ik.read_file(artifact).to_cadquery().val(), placed_body.moved(group.loc*location))['status']=='passed'
            try:
                ik.write_step_parts(source, output)
            except FileExistsError:
                pass
            else:
                raise AssertionError('Existing output was replaced')
        bad = cq.Assembly(name='Unsupported')
        bad.add(body, name='Supported')
        bad.add(cq.Solid.makeSphere(3), name='Sphere')
        _write_xde(bad, root/'unsupported.step')
        try:
            ik.write_step_parts(root/'unsupported.step', root/'rejected')
        except ik.StepPartWriteError:
            assert not (root/'rejected').exists()
        else:
            raise AssertionError('Unsupported batch was published')
    return dict(status='passed', single_parts=1, batch_occurrences=2, cli_occurrences=2,
                iam_api_occurrences=2, iam_cli_occurrences=1, iam_local_relocation=True,
                semantic_json_roundtrip=True,
                nested_placement=True, repeated_definition=True, metadata=True,
                existing_output_preserved=True, unsupported_batch_rejected=True,
                native_validation='not_run')


if __name__ == '__main__':
    print(json.dumps(installed_writer()))
