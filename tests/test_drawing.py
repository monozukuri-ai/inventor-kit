"""Experimental drawing API contracts; real-file checks are local regression evidence."""
from dataclasses import FrozenInstanceError
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from inventor_kit import read_drawing, read_drawing_file, Limits, DrawingDisplayError, DrawingLimits
from inventor_kit.viewer.scene import Options, build_scene, discard_geometry
from drawing_fixture_helpers import with_segment_major

ROOT = Path(__file__).resolve().parents[1]
SAMPLE = ROOT / 'fixtures/public/SampleBg.idw'


class DrawingAPI(unittest.TestCase):
    def test_major23_saved_sheets_views_colors_and_explicit_missing_assets(self):
        from collections import Counter
        from io import BytesIO
        from PIL import Image
        doc = read_drawing_file(ROOT/'fixtures/public/_Fishing Rod Assembly.idw')
        self.assertEqual(doc.source_sha256, 'e50760e2969eae8bb47565026fa52697d8698ec744b902bdc59fff508b1d8cdc')
        self.assertEqual(doc.status, 'experimental_partial')
        self.assertEqual([s.index for s in doc.sheets], [0, 1, 2, 3])
        self.assertEqual([len(s.views) for s in doc.sheets], [4, 9, 8, 6])
        self.assertEqual([len(s.items) for s in doc.sheets], [4257, 2296, 4464, 6922])
        self.assertEqual(len({s.id for s in doc.sheets}), 4)
        for s in doc.sheets:
            self.assertEqual(s.name, 'Sheet')
            self.assertEqual(s.size_in_source_units, (86.36, 55.88))
            self.assertEqual(s.status, 'experimental_partial')
            self.assertTrue(s.omissions)
            self.assertEqual(set(Counter(i.geometry['kind'] for i in s.items)), {'polyline', 'curve', 'text', 'image'})
            # Shaded caches precede the vector edges in their own view.
            for v in s.views:
                items = [i for i in s.items if i.placement_record == v.placement_record]
                if v.image_reference is not None:
                    self.assertEqual(items[0].geometry['kind'], 'image')
        self.assertFalse(doc.qualified or doc.complete)
        self.assertIsNone(doc.millimeters_per_unit)
        images = [i for i in doc.images if i.status == 'decoded_rgba_view_cache_unqualified']
        self.assertEqual(len(images), 22)
        for i in images:
            with Image.open(BytesIO(i.data)) as png:
                png.load()
                self.assertEqual(png.size, (i.width, i.height))
                self.assertEqual(png.mode, 'RGBA')
        missing, = [i for i in doc.images if i.data is None]
        self.assertEqual(missing.reference, 9)
        self.assertIn('No such stream', missing.diagnostic)
        # The five uncached views now include their stored spline/ellipse
        # outlines. They remain independent views even with duplicate names.
        uncached = [v for v in doc.sheets[2].views if v.image_reference is None]
        self.assertEqual([(v.placement_record, len(v.item_ids)) for v in uncached],
                         [(5, 307), (6, 414), (7, 471), (9, 706), (19, 130)])
        detail = uncached[-1]
        members = [i for i in doc.sheets[2].items if i.id in detail.item_ids]
        spline = next(i for i in members if i.record_ordinal == 12087)
        self.assertEqual(spline.geometry['kind'], 'polyline')
        self.assertGreater(len(spline.geometry['points']), 16)
        ellipse = next(i for i in members if i.record_ordinal == 12067)
        self.assertEqual(ellipse.geometry['kind'], 'curve')
        self.assertLess(ellipse.geometry['start'], ellipse.geometry['end'])
        omitted = {o['record_ordinal'] for o in doc.sheets[2].omissions}
        self.assertNotIn(spline.record_ordinal, omitted)
        self.assertNotIn(ellipse.record_ordinal, omitted)

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
        unsupported = read_drawing(with_segment_major(SAMPLE.read_bytes(), 25))
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
            scene = build_scene(path, directory, Options())
            self.assertEqual(scene['source']['kind'], 'drawing')
            self.assertEqual(scene['drawing']['status'], 'experimental_partial')
            self.assertFalse(scene['meshes'])
            self.assertEqual(scene['units'], 'source_units_unverified')
            self.assertFalse(scene['drawing']['qualified'])
            self.assertFalse(scene['drawing']['complete'])
            self.assertIsNone(scene['drawing']['millimeters_per_unit'])
            self.assertEqual(scene['drawing']['current_state'], 'unverified')
            from inventor_kit.viewer.drawing import validate_drawing_resources
            validate_drawing_resources(directory, scene)
            # The legacy flag and partial-geometry permission must not change
            # either the published bytes or the physical-unit contract.
            for options in (Options(experimental_drawing=True), Options(allow_partial=True)):
                compatible = build_scene(path, directory, options)
                self.assertEqual(compatible['drawing']['sheets'], scene['drawing']['sheets'])
                self.assertEqual(compatible['drawing']['images'], scene['drawing']['images'])
                self.assertEqual(compatible['drawing']['status'], scene['drawing']['status'])
                self.assertIsNone(compatible['drawing']['millimeters_per_unit'])
            for image in scene['drawing']['images']:
                self.assertEqual(hashlib.sha256((directory/image['resource']).read_bytes()).hexdigest(), image['sha256'])
            discard_geometry(scene)
            self.assertIsNone(scene['drawing'])
            self.assertTrue(scene['thumbnails'])
            with patch('inventor_kit.viewer.drawing.read_drawing', side_effect=AssertionError('Drawing decoder called')):
                metadata = build_scene(path, directory, Options(metadata_only=True))
            self.assertIsNone(metadata['drawing'])
            self.assertEqual(metadata['stages']['geometry'], 'not_attempted')
            self.assertTrue(metadata['thumbnails'])
            limited = build_scene(path, directory, Options(max_buffer_bytes=0))
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
    s=build_scene(p,Path(d),Options())
    assert s['drawing']['status']=='experimental_partial', s['diagnostics']
'''
        subprocess.run([sys.executable, '-c', code, str(SAMPLE)], check=True)


if __name__ == '__main__': unittest.main()
