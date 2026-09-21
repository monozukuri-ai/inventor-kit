"""Experimental drawing API contracts; real-file checks are local regression evidence."""
from dataclasses import FrozenInstanceError
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from inventor_kit import read_drawing, read_drawing_file, Limits, DrawingDisplayError, DrawingLimits
from inventor_kit.viewer.scene import Options, build_scene, discard_geometry

ROOT = Path(__file__).resolve().parents[1]
SAMPLE = ROOT / 'fixtures/public/SampleBg.idw'


class DrawingAPI(unittest.TestCase):
    def test_saved_view_ids_members_and_unknowns_survive_python_decode(self):
        import json
        from inventor_kit import _inventor, DrawingView
        from inventor_kit.drawing import _decode
        raw = json.loads(_inventor.read_drawing(SAMPLE.read_bytes(), 'test', None, None))
        space = raw['preview']['spaces'][0]
        item = space['items'][0]
        space['views'] = [dict(placement_record=item['placement_record'], name='Synthetic view',
            placement_transform=item['transform'], cache_bounds=[-2., -1., 0., 2., 1., 0.],
            image_reference=None, source=item['source'], diagnostics=['view_type_parent_rotation_and_clip_unverified'])]
        doc = _decode(raw)
        view, = doc.sheets[0].views
        self.assertIsInstance(view, DrawingView)
        self.assertTrue(view.id.startswith(doc.sheets[0].id + '/view-'))
        self.assertIn(doc.sheets[0].items[0].id, view.item_ids)
        self.assertIsNone(view.rotation)
        self.assertIsNone(view.parent_view_id)
        self.assertIsNone(view.view_type)
        with self.assertRaises(FrozenInstanceError): view.name = 'changed'

    def test_drawing_limits_match_native_and_reject_invalid_options_before_input(self):
        import json
        from dataclasses import asdict, fields
        from inventor_kit import _inventor
        defaults = DrawingLimits()
        self.assertEqual(asdict(defaults), json.loads(_inventor.default_drawing_limits()))
        schema = json.loads((ROOT/'schemas/drawing-scene-v1.schema.json').read_text())['$defs']['drawing_limits']
        self.assertEqual(asdict(defaults), {k:v['maximum'] for k,v in schema['properties'].items()})
        for field in fields(defaults):
            for value in (-1, True, 1.5, field.default + 1):
                with self.subTest(field=field.name, value=value), self.assertRaises(ValueError):
                    DrawingLimits(**{field.name:value})
        with self.assertRaises(TypeError): read_drawing_file('/nonexistent.idw', drawing_limits={})
        with self.assertRaisesRegex(ValueError, 'drawing.*hard limit'):
            _inventor.read_drawing(b'', 'invalid', None, '{"max_sheets":257}')

    def test_native_drawing_budgets_drop_uncommitted_display_and_keep_metadata(self):
        for field in ('max_sheets', 'max_display_items', 'max_polyline_points', 'max_text_bytes',
                      'max_reference_visits', 'max_nesting_depth'):
            with self.subTest(field=field):
                doc = read_drawing_file(SAMPLE, drawing_limits=DrawingLimits(**{field:0}))
                self.assertEqual(doc.status, 'unavailable')
                self.assertTrue(doc.metadata.thumbnails)
                self.assertFalse(any(s.items for s in doc.sheets))
                self.assertTrue(doc.diagnostics)
        for field in ('max_image_bytes', 'max_image_pixels'):
            doc = read_drawing_file(SAMPLE, drawing_limits=DrawingLimits(**{field:0}))
            self.assertEqual(len(doc.images), 2)
            self.assertTrue(all(i.data is None and 'limit' in i.diagnostic for i in doc.images))
        with self.assertRaisesRegex(ValueError, 'drawing output byte limit'):
            read_drawing_file(SAMPLE, drawing_limits=DrawingLimits(max_output_bytes=1))
        # An item budget covers the entire expansion, not just each primitive.
        exact = read_drawing_file(SAMPLE, drawing_limits=DrawingLimits(max_display_items=157))
        self.assertEqual(len(exact.sheets[0].items),157)
        short = read_drawing_file(SAMPLE, drawing_limits=DrawingLimits(max_display_items=156))
        self.assertEqual(short.status,'unavailable')


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
        self.assertTrue(sheet.id.startswith(doc.source_sha256 + '/'))
        for partial in (False, True):
            with self.assertRaises(DrawingDisplayError) as caught:
                doc.render_sheet(sheet_id=sheet.id, allow_partial=partial)
            self.assertEqual(caught.exception.sheet_id, sheet.id)
            self.assertIn('drawing.units_unverified', [d['code'] for d in caught.exception.diagnostics])
        with self.assertRaises(KeyError): doc.sheet('Blatt')
        with self.assertRaises(DrawingDisplayError): doc.render_sheet(sheet_id='Blatt')
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

    def test_ids_bind_input_bytes_but_not_path(self):
        data = SAMPLE.read_bytes()
        first = read_drawing(data, source_id='one/path.idw')
        moved = read_drawing(data, source_id='other/path.idw')
        self.assertEqual(first.sheets[0].id, moved.sheets[0].id)
        # A CFB with an unused trailing sector is a distinct immutable input.
        other = read_drawing(data + bytes(512))
        self.assertEqual(other.sheets[0].name, first.sheets[0].name)
        with self.assertRaises(KeyError):
            other.sheet(first.sheets[0].id)

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
            self.assertEqual(disabled['drawing']['status'], 'unavailable')
            self.assertIsNone(disabled['drawing']['sheets'][0]['resource'])
            self.assertTrue(any(d['code'] == 'drawing.units_unverified' for d in disabled['diagnostics']))
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
