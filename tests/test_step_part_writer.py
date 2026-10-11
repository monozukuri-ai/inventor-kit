"""Synthetic batch tests; native qualification remains a separate private gate."""
from dataclasses import replace
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import cadquery as cq
import inventor_kit as ik
from inventor_kit.assembly_step import _write_xde
from inventor_kit.step_parts import _compare, _publish_directory, _read_parts


class StepParts(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.source = self.root / '入力 部品.step'
        self.output = self.root / '部品一式'
        self.box = cq.Solid.makeBox(2, 3, 5)

    def tearDown(self):
        self.tmp.cleanup()

    def source_tree(self, shapes=None):
        assembly = cq.Assembly(name='Assembly')
        for i, s in enumerate(shapes or [self.box]):
            assembly.add(s, name=f'Part{i}')
        _write_xde(assembly, self.source)
        # This fixture exporter mis-encodes non-ASCII XDE names. Construct valid
        # ISO 10303 Unicode escapes in the source, independently of our reader.
        data = self.source.read_text()
        for i in range(len(shapes or [self.box])):
            escaped = '\\X2\\' + f'部品{i}'.encode('utf-16-be').hex().upper() + '\\X0\\'
            data = data.replace(f"'Part{i}'", f"'{escaped}'")
        self.source.write_text(data)

    def test_nested_placements_repeated_definitions_and_world_geometry(self):
        assembly = cq.Assembly(name='Root')
        group_loc = cq.Location((101, -7, 3), (0, 0, 1), 37)
        inner_loc = cq.Location((2, 11, 5), (0, 1, 0), 23)
        a_loc, b_loc = cq.Location((7, 0, 2)), cq.Location((-5, 2, 1))
        group = cq.Assembly(name='Group', loc=group_loc)
        inner = cq.Assembly(name='Inner', loc=inner_loc)
        inner.add(self.box, name='A', loc=a_loc)
        group.add(inner)
        group.add(self.box, name='B', loc=b_loc)
        assembly.add(group)
        _write_xde(assembly, self.source)
        result = ik.write_step_parts(self.source, self.output)
        report = json.loads(result.report_path.read_text())
        self.assertEqual(result.part_count, 2)
        a, b = report['parts']
        self.assertEqual(a['definition'], b['definition'])
        self.assertNotEqual(a['artifact']['path'], b['artifact']['path'])
        for row, shape in zip(report['parts'], [self.box.moved(group_loc * inner_loc * a_loc),
                                              self.box.moved(group_loc * b_loc)]):
            actual = ik.read_file(self.output / row['artifact']['path']).to_cadquery().val()
            self.assertEqual(_compare(actual, shape)['status'], 'passed')
        self.assertFalse(report['geometry_deduplication'])
        self.assertEqual(report['native_validation'], 'not_run')

    def test_already_positioned_geometry_is_not_moved_twice_and_names_survive(self):
        self.source_tree([self.box.translate((123, -57, 9))])
        result = ik.write_step_parts(self.source, self.output)
        row = json.loads(result.report_path.read_text())['parts'][0]
        self.assertEqual(row['name'], '部品0')
        doc = ik.read_file(self.output / row['artifact']['path'])
        self.assertEqual(_compare(doc.to_cadquery().val(), self.box.translate((123, -57, 9)))['status'], 'passed')
        props = {p.semantic_name: p.value for s in doc.metadata.property_sets for p in s.properties if p.semantic_name}
        self.assertEqual(props, dict(title='部品0', part_number='部品0'))
        self.assertEqual(len(list(self.output.iterdir())), 3)
        sidecar = json.loads((self.output / row['report']['path']).read_text())
        self.assertEqual(sidecar['artifact']['path'], row['artifact']['path'])

    def test_unsupported_second_part_publishes_nothing_and_identifies_failure(self):
        self.source_tree([self.box, cq.Solid.makeSphere(3)])
        with self.assertRaises(ik.StepPartWriteError) as caught:
            ik.write_step_parts(self.source, self.output)
        self.assertEqual(caught.exception.report['phase'], 'encode')
        self.assertEqual(caught.exception.report['part'][-1], '部品1')
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.iterdir()), [self.source])

    def test_existing_empty_directory_file_and_symlink_preserved(self):
        self.source_tree()
        for kind in ('directory', 'file', 'symlink'):
            with self.subTest(kind=kind):
                if kind == 'directory': self.output.mkdir()
                elif kind == 'file': self.output.write_bytes(b'keep')
                else: self.output.symlink_to(self.root / 'absent')
                with self.assertRaises(FileExistsError):
                    ik.write_step_parts(self.source, self.output)
                if kind == 'directory': self.output.rmdir()
                else: self.output.unlink()

    def test_publication_race_does_not_replace_competing_directory(self):
        self.source_tree()
        def competing(stage, destination):
            destination.mkdir()
            _publish_directory(stage, destination)
        with patch('inventor_kit.step_parts._publish_directory', side_effect=competing):
            with self.assertRaises(ik.StepPartWriteError) as caught:
                ik.write_step_parts(self.source, self.output)
        self.assertEqual(caught.exception.report['phase'], 'publish')
        self.assertTrue(self.output.is_dir())
        self.assertEqual(list(self.output.iterdir()), [])

    def test_filename_normalization_preserves_colliding_and_reserved_names(self):
        assembly = cq.Assembly(name='Root')
        for i, name in enumerate(['A:B', 'A?B', 'CON', 'con', 'Trailing.']):
            # Distinct solids prevent the exporter from intentionally sharing the definition name.
            assembly.add(cq.Solid.makeBox(2+i, 3, 5), name=name)
        _write_xde(assembly, self.source)
        result = ik.write_step_parts(self.source, self.output)
        rows = json.loads(result.report_path.read_text())['parts']
        self.assertEqual([r['name'] for r in rows], ['A:B', 'A?B', 'CON', 'con', 'Trailing.'])
        filenames = [r['artifact']['path'] for r in rows]
        self.assertEqual(len(set(p.casefold() for p in filenames)), 5)
        for filename in filenames:
            self.assertNotRegex(filename, r'[<>:"/\\|?*]')

    def test_limits_invalid_source_units_and_mixed_geometry_fail_closed(self):
        self.source_tree([self.box, cq.Solid.makeBox(4, 5, 6)])
        defaults = ik.StepPartLimits()
        for kwargs in ({'max_file_bytes': 1}, {'max_entities': 1}, {'max_parts': 1},
                       {'max_nodes': 1}, {'max_depth': 1}, {'max_total_topology': 1}, {'max_output_bytes': 1}):
            with self.subTest(limits=kwargs), self.assertRaises(ik.StepPartWriteError):
                ik.write_step_parts(self.source, self.output, limits=replace(defaults, **kwargs))
            self.assertFalse(self.output.exists())
        data = self.source.read_text()
        self.assertIn('SI_UNIT(.MILLI.,.METRE.)', data)
        self.source.write_text(data.replace('SI_UNIT(.MILLI.,.METRE.)', 'SI_UNIT(.CENTI.,.METRE.)'))
        with self.assertRaisesRegex(ik.StepPartWriteError, 'millimetre'):
            ik.write_step_parts(self.source, self.output)
        self.source.write_text('corrupt STEP')
        with self.assertRaises(ik.StepPartWriteError):
            ik.write_step_parts(self.source, self.output)
        self.assertFalse(self.output.exists())
        mixed = cq.Compound.makeCompound([self.box, cq.Face.makePlane(2, 3)])
        self.source_tree([mixed])
        with self.assertRaises(ik.StepPartWriteError):
            ik.write_step_parts(self.source, self.output)
        self.assertFalse(self.output.exists())

    def test_changed_source_prevents_publication(self):
        self.source_tree()
        original = _read_parts
        def changed(path, limits):
            result = original(path, limits)
            self.source.write_bytes(b'changed by another process')
            return result
        with patch('inventor_kit.step_parts._read_parts', side_effect=changed):
            with self.assertRaisesRegex(ik.StepPartWriteError, 'changed'):
                ik.write_step_parts(self.source, self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(self.source.read_bytes(), b'changed by another process')

    def test_equal_metrics_do_not_hide_a_moved_hole(self):
        base = cq.Workplane('XY').box(20, 12, 4, centered=False).val()
        shapes = [base.cut(cq.Workplane('XY').center(x, 6).circle(2).extrude(4).val()).Solids()[0]
                  for x in (6, 14)]
        self.assertAlmostEqual(shapes[0].Volume(), shapes[1].Volume(), places=7)
        self.assertAlmostEqual(shapes[0].Area(), shapes[1].Area(), places=7)
        with self.assertRaises(ik.PartWriteError):
            _compare(*shapes)

    def test_invalid_boolean_result_is_never_accepted(self):
        from unittest.mock import MagicMock
        invalid = MagicMock()
        invalid.isValid.return_value = False
        invalid.Volume.return_value = 0.0
        with patch.object(cq.Shape, 'cut', return_value=invalid):
            with self.assertRaisesRegex(ik.PartWriteError, 'invalid shape'):
                _compare(self.box, self.box.copy())


if __name__ == '__main__':
    unittest.main()
