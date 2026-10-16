"""Output version boundaries and geometry. Local tests are not native approval."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import cadquery as cq
import inventor_kit as ik
from inventor_kit import _inventor
from inventor_kit.writer import ANALYTIC_PROFILE, ANALYTIC_PROFILE_2026

class WriterVersions(unittest.TestCase):
    def test_invalid_and_conflicting_targets_fail_before_writing(self):
        for year in (2025, 2028, 0, True, '2026', 2026.0):
            with self.subTest(year=year), self.assertRaises(ik.PartWriteError):
                ik.new_part(name='invalid', target_year=year)
            with self.subTest(year=year), self.assertRaises(ik.AssemblyWriteError):
                ik.new_assembly(name='invalid', target_year=year)
        with self.assertRaises(ik.PartWriteError):
            ik.new_part(name='conflict', target_year=2026, target_profile=ANALYTIC_PROFILE)
        self.assertEqual(ik.new_part(name='default')._target_profile, ANALYTIC_PROFILE)
        self.assertEqual(ik.new_part(name='profile', target_profile=ANALYTIC_PROFILE_2026)._target_year, 2026)

    def test_both_versions_geometry_mixed_inputs_and_relocated_references(self):
        box = cq.Workplane('XY').box(20, 30, 10, centered=False).val()
        cylinder = cq.Workplane('XY').center(5, 5).circle(2).extrude(10).val()
        shapes = [box, cylinder, box.cut(cylinder).Solids()[0]]
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            all_parts = {}
            for year in (2026, 2027):
                with self.subTest(year=year):
                    parts = []
                    for i, shape in enumerate(shapes):
                        report = ik.new_part(name=f'部品{i}', target_year=year).add_body(shape).saveas(root/f'{year}-{i}.ipt')
                        restored = ik.read_file(report.path).to_cadquery().val()
                        self.assertTrue(restored.isValid())
                        self.assertEqual(len(restored.Faces()), len(shape.Faces()))
                        self.assertAlmostEqual(restored.Volume(), shape.Volume(), places=6)
                        self.assertLess(restored.cut(shape).Volume() + shape.cut(restored).Volume(), 1e-5)
                        identity = json.loads(_inventor.inspect_authored_part(report.path.read_bytes(), report.path.name))
                        self.assertEqual(identity['target_year'], year)
                        sidecar = json.loads(report.report_path.read_text())
                        self.assertEqual(sidecar['target_year'], year)
                        self.assertEqual(sidecar['native_validation'], 'not_run')
                        self.assertEqual(sidecar['profile_qualification'], 'native-qualified-bounded-corpus')
                        parts.append(report.path)
                    all_parts[year] = parts
                    asm = ik.new_assembly(name='組立', target_year=year)
                    for part in parts:
                        asm.add_part(part)
                    asm.add_part(parts[0], name='repeat')
                    result = asm.save_bundle(root/f'assembly-{year}', filename='組立.iam')
                    self.assertIn(f'major{year-1996}' if year == 2026 else 'rse31', result.target_profile)
                    moved = root/f'移動先 {year}'
                    result.directory.rename(moved)
                    scene = ik.read_assembly_file(moved/'組立.iam')
                    self.assertEqual(len(scene.instances), 4)
                    bundle_report = json.loads((moved/'bundle.json').read_text())
                    self.assertEqual(bundle_report['target_year'], year)
                    self.assertTrue(bundle_report['document']['native_qualified'])
                    self.assertEqual(bundle_report['native_validation'], 'not_run')
            for year, other in ((2026, 2027), (2027, 2026)):
                asm = ik.new_assembly(name='bad', target_year=year)
                with self.assertRaisesRegex(ik.AssemblyWriteError, 'target versions must match'):
                    asm.add_part(all_parts[other][0])
                self.assertEqual(len(asm._parts), 0)
                spec = dict(name='bad', part_number='bad', target_year=year, occurrences=[dict(part=0, name='Part:1')])
                with self.assertRaisesRegex(ValueError, 'target versions must match'):
                    _inventor.encode_fixed_assembly(json.dumps(spec), [('box.ipt', all_parts[other][0].read_bytes())])

    def test_cli_step_to_iam_propagates_target_version(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            shape = cq.Workplane('XY').box(10, 20, 30).val()
            source = root/'input.step'
            cq.exporters.export(shape, str(source))
            command = [sys.executable, '-m', 'inventor_kit']
            for year in (2026, 2027):
                parts = root/f'parts-{year}'
                result = subprocess.run(command+['write-step', str(source), str(parts), '--target-year', str(year)], capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
                self.assertEqual(json.loads(result.stdout)['target_year'], year)
                self.assertEqual(json.loads((parts/'batch.json').read_text())['target_year'], year)
                bundle = root/f'bundle-{year}'
                result = subprocess.run(command+['write-assembly', str(bundle), *map(str, parts.glob('*.ipt')), '--target-year', str(year)], capture_output=True, text=True, timeout=120)
                self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
                self.assertEqual(json.loads(result.stdout)['target_year'], year)
                self.assertEqual(len(ik.read_assembly_file(bundle/'assembly.iam').instances), 1)

if __name__ == '__main__': unittest.main()
