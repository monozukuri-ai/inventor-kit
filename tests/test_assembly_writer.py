"""Public synthetic IAM tests. Local results are not native acceptance evidence."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import inventor_kit as ik
from inventor_kit import _inventor


class AssemblyWriter(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import cadquery as cq
        cls.shared = tempfile.TemporaryDirectory()
        cls.source = Path(cls.shared.name)
        cls.box = ik.new_part(name='Box').add_body(cq.Workplane('XY').box(20, 30, 10, centered=False).val()).saveas(cls.source/'box.ipt').path
        cls.cylinder = ik.new_part(name='Cylinder').add_body(cq.Workplane('XY').center(40, 0).circle(5).extrude(15).val()).saveas(cls.source/'cylinder.ipt').path

    @classmethod
    def tearDownClass(cls):
        cls.shared.cleanup()

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_multiple_definitions_repeats_metadata_and_portability(self):
        builder = ik.new_assembly(name='組立', part_number='ASM-01')
        builder.add_part(self.box, name='Box:1').add_part(self.cylinder).add_part(self.box, name='Box:2')
        result = builder.save_bundle(self.root/'before', filename='組立.iam')
        self.assertEqual((result.definition_count, result.occurrence_count), (2, 3))
        self.assertEqual(result.native_validation, 'not_run')
        report = json.loads(result.report_path.read_text())
        self.assertEqual([o['reference_id'] for o in report['document']['occurrences']], [1, 2, 1])
        self.assertEqual(report['native_validation'], 'not_run')
        for entry in [report['assembly'], *report['parts']]:
            self.assertEqual(entry['sha256'], hashlib.sha256((result.directory/entry['path']).read_bytes()).hexdigest())
        props = ik.inspect_file(result.path).metadata.property_sets
        self.assertEqual(next(p.value for s in props for p in s.properties if p.semantic_name == 'part_number'), 'ASM-01')
        moved = self.root/'移動先 folder'
        result.directory.rename(moved)
        parsed = ik.inspect_assembly_file(moved/'組立.iam')
        self.assertEqual(len(parsed.occurrences), 3)
        scene = ik.read_assembly_file(moved/'組立.iam')
        self.assertEqual(len(scene.instances), 3)
        # No source directory resolver override is required after bundle move.
        self.assertTrue(all((moved/e['path']).is_file() for e in report['parts']))

    def test_binding_is_a_snapshot_and_each_save_has_fresh_identity(self):
        source = self.root/'bound.ipt'
        original = self.box.read_bytes()
        source.write_bytes(original)
        builder = ik.new_assembly(name='Snapshot').add_part(source)
        source.unlink()
        a = builder.save_bundle(self.root/'one')
        b = builder.save_bundle(self.root/'two')
        self.assertEqual((a.directory/'bound.ipt').read_bytes(), original)
        ra, rb = (json.loads(p.report_path.read_text()) for p in (a, b))
        self.assertNotEqual(ra['document']['document_id'], rb['document']['document_id'])
        self.assertNotEqual(a.sha256, b.sha256)

    def test_rejects_paths_names_aliases_and_unsupported_input(self):
        for name in ('', '\0', 'a\n', 'x'*256, '\ud800'):
            with self.subTest(name=name), self.assertRaises(ik.AssemblyWriteError):
                ik.new_assembly(name=name)
        for filename in ('../box.ipt', 'sub/box.ipt', 'CON.ipt', 'a\\b.ipt', 'LPT².ipt', 'a:stream.ipt', 'a.iam'):
            with self.subTest(filename=filename), self.assertRaises(ik.AssemblyWriteError):
                ik.new_assembly(name='bad').add_part(self.box, filename=filename)
        builder = ik.new_assembly(name='aliases').add_part(self.box, name='Part')
        with self.assertRaises(ik.AssemblyWriteError):
            builder.add_part(self.box, name='part')
        with self.assertRaises(ik.AssemblyWriteError):
            builder.add_part(self.cylinder, filename='BOX.ipt')
        self.assertEqual(len(builder._occurrences), 1)
        corrupt = self.root/'corrupt.ipt'
        corrupt.write_bytes(b'not a part')
        with self.assertRaises(ik.AssemblyWriteError):
            builder.add_part(corrupt)
        directory = self.root/'directory.ipt'
        directory.mkdir()
        with self.assertRaises(ik.AssemblyWriteError):
            builder.add_part(directory)
        with self.assertRaises(ik.AssemblyWriteError):
            ik.new_assembly(name='empty').save_bundle(self.root/'empty')
        self.assertFalse((self.root/'empty').exists())

    def test_existing_directory_and_publication_race_preserve_other_work(self):
        target = self.root/'bundle'
        target.mkdir()
        builder = ik.new_assembly(name='race').add_part(self.box)
        with self.assertRaises(FileExistsError):
            builder.save_bundle(target)
        target.rmdir()
        from inventor_kit.step_parts import _publish_directory
        def race(staging, destination):
            destination.mkdir()
            (destination/'keep').write_text('other writer')
            _publish_directory(staging, destination)
        with patch('inventor_kit.step_parts._publish_directory', side_effect=race), self.assertRaises(FileExistsError):
            builder.save_bundle(target)
        self.assertEqual(list(target.iterdir()), [target/'keep'])
        self.assertEqual((target/'keep').read_text(), 'other writer')

    def test_rust_boundary_rejects_unqualified_options_and_unused_inputs(self):
        spec = dict(name='test', part_number='test', occurrences=[dict(part=0, name='Box:1')])
        parts = [('box.ipt', self.box.read_bytes())]
        for bad in ({**spec, 'template': 'opaque'},
                    {**spec, 'occurrences': [dict(part=0, name='X', matrix_mm=[[1]])]},
                    {**spec, 'occurrences': [dict(part=1, name='X')]},
                    {**spec, 'occurrences': []}):
            with self.subTest(spec=bad), self.assertRaises(ValueError):
                _inventor.encode_fixed_assembly(json.dumps(bad), parts)
        with self.assertRaises(ValueError):
            _inventor.encode_fixed_assembly(json.dumps(spec), parts+[('cylinder.ipt', self.cylinder.read_bytes())])

    def test_cli_success_and_failure_publish_only_successful_bundle(self):
        command = [sys.executable, '-m', 'inventor_kit', 'write-assembly']
        target = self.root/'cli'
        result = subprocess.run(command+[str(target), str(self.box), str(self.cylinder)], capture_output=True, text=True, timeout=120)
        self.assertEqual(result.returncode, 0, result.stderr+result.stdout)
        report = json.loads(result.stdout)
        self.assertTrue(report['published'])
        self.assertEqual(report['native_validation'], 'not_run')
        before = (target/'assembly.iam').read_bytes()
        result = subprocess.run(command+[str(target), str(self.box)], capture_output=True, text=True, timeout=30)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(json.loads(result.stdout)['published'])
        self.assertEqual((target/'assembly.iam').read_bytes(), before)

    def test_cli_worker_failure_cannot_publish_staged_bundle(self):
        from inventor_kit.assembly_writer_cli import write_job
        for mode in ('timeout', 'crash'):
            target = self.root/mode
            def failed_worker(command, **kwargs):
                request = json.loads(Path(command[-2]).read_text())
                staging = Path(request['staging'])
                staging.mkdir()
                (staging/'partial.iam').write_bytes(b'incomplete')
                if mode == 'timeout':
                    raise subprocess.TimeoutExpired(command, kwargs['timeout'])
                return subprocess.CompletedProcess(command, 1)
            with self.subTest(mode=mode), patch('inventor_kit.assembly_writer_cli.subprocess.run', side_effect=failed_worker):
                report = write_job([self.box], target)
                self.assertEqual(report['status'], 'error')
                self.assertFalse(report['published'])
                self.assertFalse(target.exists())


if __name__ == '__main__':
    unittest.main()
