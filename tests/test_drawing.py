"""Experimental drawing API contracts; real-file checks are local regression evidence."""
from dataclasses import FrozenInstanceError
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from inventor_kit import read_drawing, read_drawing_file, Limits
from inventor_kit.viewer.scene import Options, build_scene, discard_geometry

ROOT = Path(__file__).resolve().parents[1]
SAMPLE = ROOT / 'fixtures/public/SampleBg.idw'


class DrawingAPI(unittest.TestCase):
    def test_saved_sheet_identity_units_and_immutable_elements(self):
        doc = read_drawing_file(SAMPLE)
        self.assertEqual(doc.source_sha256, hashlib.sha256(SAMPLE.read_bytes()).hexdigest())
        self.assertEqual(doc.status, 'experimental_partial')
        self.assertFalse(doc.qualified or doc.complete)
        self.assertIsNone(doc.length_unit)
        self.assertIsNone(doc.millimeters_per_unit)
        self.assertEqual(doc.current_state, 'unverified')
        sheet, = doc.sheets
        self.assertEqual(sheet.name, 'Blatt')
        self.assertEqual(sheet.size_in_source_units, (42., 29.7))
        self.assertEqual(len(sheet.items), 157)
        self.assertEqual(len({i.id for i in sheet.items}), 157)
        self.assertEqual(doc.sheet(sheet.id), sheet)
        with self.assertRaises(KeyError): doc.sheet('Blatt')
        with self.assertRaises(FrozenInstanceError): sheet.name = 'changed'
        with self.assertRaises(TypeError): sheet.items[0].geometry['kind'] = 'changed'
        self.assertEqual(len(doc.images), 2)
        for image in doc.images:
            self.assertEqual(hashlib.sha256(image.data).hexdigest(), image.sha256)
        again = read_drawing_file(SAMPLE)
        self.assertEqual([i.id for i in again.sheets[0].items], [i.id for i in sheet.items])

    def test_kind_limits_and_unsupported_major(self):
        for data in (b'broken', (ROOT/'fixtures/public/SamplePart.ipt').read_bytes()):
            with self.assertRaises(ValueError): read_drawing(data)
        with self.assertRaisesRegex(ValueError, 'file byte limit'):
            read_drawing(SAMPLE.read_bytes(), limits=Limits(max_file_bytes=1))
        unsupported = read_drawing_file(ROOT/'fixtures/public/drawings/iacs/Template_IACS.idw')
        self.assertEqual(unsupported.status, 'unavailable')
        self.assertFalse(unsupported.sheets)
        self.assertTrue(unsupported.metadata.thumbnails)
        self.assertTrue(unsupported.diagnostics)

    def test_viewer_dispatch_uses_root_identity_and_rejects_geometry_options(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            path = directory/'renamed.ipt'
            path.write_bytes(SAMPLE.read_bytes())
            scene = build_scene(path, directory, Options(experimental_drawing=True))
            self.assertEqual(scene['source']['kind'], 'drawing')
            self.assertEqual(scene['drawing']['status'], 'experimental_partial')
            self.assertFalse(scene['meshes'])
            self.assertEqual(scene['units'], 'source_units_unverified')
            for image in scene['drawing']['images']:
                self.assertEqual(hashlib.sha256((directory/image['resource']).read_bytes()).hexdigest(), image['sha256'])
            discard_geometry(scene)
            self.assertIsNone(scene['drawing'])
            self.assertTrue(scene['thumbnails'])
            disabled = build_scene(path, directory, Options())
            self.assertIsNone(disabled['drawing'])
            self.assertEqual(disabled['stages']['geometry'], 'not_enabled')
            limited = build_scene(path, directory, Options(experimental_drawing=True, max_buffer_bytes=0))
            self.assertIsNone(limited['drawing'])
            self.assertEqual(limited['stages']['geometry'], 'failed')
            self.assertTrue(limited['thumbnails'])
        for option in ('allow_partial','metadata_only','require_current_state','allow_unverified_state'):
            with self.assertRaises(ValueError): Options(experimental_drawing=True, **{option: True})

    def test_no_optional_geometry_import_for_real_drawing(self):
        code = '''
import sys
class Block:
    def find_spec(self, fullname, path=None, target=None):
        if fullname.split('.')[0] in {'cadquery','cq_acis','ocp_tessellate','OCP'}:
            raise AssertionError(fullname)
sys.meta_path.insert(0, Block())
from inventor_kit import read_drawing_file
from inventor_kit.viewer.scene import Options,build_scene
from tempfile import TemporaryDirectory
from pathlib import Path
p=Path(sys.argv[1])
assert len(read_drawing_file(p).sheets[0].items)==157
with TemporaryDirectory() as d:
    s=build_scene(p,Path(d),Options(experimental_drawing=True))
    assert s['drawing']['status']=='experimental_partial', s['diagnostics']
'''
        subprocess.run([sys.executable, '-c', code, str(SAMPLE)], check=True)


if __name__ == '__main__': unittest.main()
