"""STEP transport checks: synthetic assemblies and unchanged native IAM bytes."""
from copy import deepcopy
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import cadquery as cq
import inventor_kit as ik
from inventor_kit.assembly import AssemblyConversion, AssemblyOmission
from inventor_kit.assembly_step import _compare

ROOT = Path(__file__).resolve().parents[1]


def colored_tree():
    part = cq.Workplane('XY').box(2, 3, 5).val()
    root = cq.Assembly(name='Root')
    nested = cq.Assembly(name='Nested', loc=cq.Location((11, 2, 3), (0, 0, 1), 90))
    inner = cq.Assembly(name='Inner', loc=cq.Location((0, 4, 0), (0, 1, 0), 45))
    inner.add(part, name='Repeated A', color=cq.Color(.2, .4, .6), loc=cq.Location((2, 0, 1)))
    nested.add(inner)
    nested.add(part, name='Repeated B', color=cq.Color(.2, .4, .6), loc=cq.Location((5, 0, 0)))
    root.add(nested)
    # A different appearance is a distinct STEP definition of the same geometry.
    root.add(part, name='Red', color=cq.Color(.8, .1, .2))
    return AssemblyConversion(root, (), 3, 1)


class AssemblySTEP(unittest.TestCase):
    def test_hierarchy_noncommuting_transforms_names_rgb_and_sharing(self):
        result = colored_tree()
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'tree.step'
            report = result.export_step(path)
            self.assertEqual(report['roundtrip']['status'], 'passed')
            self.assertEqual(report['roundtrip']['part_instances'], 3)
            self.assertEqual(report['roundtrip']['part_definitions'], 2)
            self.assertEqual(report['roundtrip']['occurrences'], 5)
            self.assertEqual(report['step_sha256'], hashlib.sha256(path.read_bytes()).hexdigest())
            disk = json.loads(path.with_suffix('.step.json').read_text())
            self.assertFalse(disk['complete'])
            self.assertEqual(disk['vendor_comparison'], 'not_collected')
            a, b = [n for n in report['actual'] if n['path'][-1].startswith('Repeated')]
            self.assertEqual(a['definition'], b['definition'])
            self.assertNotEqual(a['world_transform_mm'], b['world_transform_mm'])
            self.assertEqual(a['path'], ('Root', 'Nested', 'Inner', 'Repeated A'))
            # Independent point transforms through CadQuery locations verify the
            # source snapshot's multiplication order, in addition to STEP readback.
            point = cq.Vector(0, 0, 0)
            expected = (result.assembly.loc * result.assembly.children[0].loc *
                        result.assembly.children[0].children[0].loc *
                        result.assembly.children[0].children[0].children[0].loc)
            point = point.transform(cq.Matrix(expected.wrapped.Transformation()))
            for value, row in zip(point.toTuple(), a['world_transform_mm'][:3]):
                self.assertAlmostEqual(value, row[3], places=7)
            with self.assertRaises(FileExistsError):
                result.export_step(path)
            self.assertEqual(report['step_sha256'], hashlib.sha256(path.read_bytes()).hexdigest())

    def test_comparison_detects_loss_in_each_contract_dimension(self):
        with tempfile.TemporaryDirectory() as temp:
            report = colored_tree().export_step(Path(temp) / 'tree.step')
        expected = report['expected']
        actual = report['actual']
        part = next(i for i, n in enumerate(actual) if n['kind'] == 'part')
        mutations = [
            lambda ns: ns.pop(),
            lambda ns: ns[part].update(path=(*ns[part]['path'][:-1], 'renamed')),
            lambda ns: ns[part]['local_transform_mm'][0].__setitem__(3, 99),
            lambda ns: ns[part]['world_transform_mm'][0].__setitem__(3, 99),
            lambda ns: ns[part].update(color_rgb=[1., 0., 0.]),
            lambda ns: ns[part].update(definition='split_definition'),
            lambda ns: ns[part]['geometry'].update(volume_mm3=0),
            lambda ns: ns[part]['geometry'].update(valid=False),
            lambda ns: ns.append(deepcopy(ns[part])),
        ]
        for mutate in mutations:
            with self.subTest(mutation=mutate):
                changed = deepcopy(actual)
                mutate(changed)
                self.assertEqual(_compare(expected, changed)['status'], 'failed')

    def test_loss_report_partial_optin_and_failed_transfer_creates_no_outputs(self):
        result = replace(colored_tree(), omissions=(AssemblyOmission(3, (4,), 'missing', 'missing.ipt'),),
                         reference_issues=({'reference_id': 4, 'status': 'missing'},),
                         source_documents=({'sha256': '0' * 64, 'path': 'synthetic.iam'},))
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'partial.stp'
            with self.assertRaisesRegex(ValueError, 'allow_partial'):
                result.export_step(path)
            with patch('inventor_kit.assembly_step._read_xde', return_value=[]):
                with self.assertRaisesRegex(ValueError, 'round-trip failed'):
                    result.export_step(path, allow_partial=True)
            self.assertEqual(list(Path(temp).iterdir()), [])
            report = result.export_step(path, allow_partial=True)
            self.assertEqual(report['omissions'][0]['reason'], 'missing')
            self.assertEqual(report['source_documents'][0]['path'], 'synthetic.iam')

    def test_unsupported_appearance_is_rejected_before_export(self):
        for change in ('group_color', 'alpha', 'subshape', 'root_location'):
            result = colored_tree()
            leaf = result.assembly.children[-1]
            if change == 'group_color':
                result.assembly.color = cq.Color('red')
            elif change == 'alpha':
                leaf.color = cq.Color(.2, .4, .6, .5)
            elif change == 'subshape':
                leaf.addSubshape(leaf.obj.Faces()[0], color=cq.Color('red'))
            else:
                result.assembly.loc = cq.Location((1, 0, 0))
            with tempfile.TemporaryDirectory() as temp:
                with self.assertRaises(ValueError):
                    result.export_step(Path(temp) / 'rejected.step')
                self.assertEqual(list(Path(temp).iterdir()), [])

    def test_native_subassembly_roundtrip_keeps_unknown_state_and_sources(self):
        path = ROOT / 'fixtures/public/m5-samplebg/Subassembly.iam'
        if not path.exists():
            self.skipTest('Run scripts/fetch_assembly_samples.py')
        result = ik.read_assembly_file(path).to_cadquery(allow_unverified_state=True)
        with tempfile.TemporaryDirectory() as temp:
            report = result.export_step(Path(temp) / 'native.step')
        self.assertEqual(report['roundtrip']['part_instances'], 1)
        self.assertEqual(report['current_state'], 'unverified')
        self.assertEqual(len(report['source_documents']), 2)
        self.assertTrue(all(n['color_rgb'] is None for n in report['actual']))


if __name__ == '__main__':
    unittest.main()
