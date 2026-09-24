"""Pinned real-file regression for added profiles, without native accuracy claims."""
from io import BytesIO
import json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

import jsonschema
from PIL import Image
from inventor_kit import read_drawing, read_drawing_file, DrawingDisplayError, DrawingLimits
from inventor_kit.viewer.scene import Options, build_scene
from inventor_kit.viewer.drawing import _unpack_styles, validate_drawing_resources
from inventor_kit.drawing_output import item_dict

ROOT = Path(__file__).resolve().parents[1]
CASES = [
    (24, 'iacs/Template_IACS', 70, 0),
    (29, 'versions/Toys-R-Us-Rex', 166, 3),
    (28, 'versions/mateolikescats', 1650, 5),
    (26, 'versions/RespiraWorks', 543, 3),
    (26, 'versions/starliliko', 90, 0),
    (26, 'versions/RespiraWorks-bottom-assembly', 550, 3),
    (26, 'versions/RespiraWorks-filter-panel-assembly', 28820, 4),
]


class DrawingVersions(unittest.TestCase):
    def test_dense_curves_have_a_separate_bounded_field_budget(self):
        path = ROOT/'fixtures/public/drawings/versions/RespiraWorks-filter-panel-assembly.idw'
        short = read_drawing_file(path, drawing_limits=DrawingLimits(max_field_values=500000))
        self.assertEqual(short.status, 'unavailable')
        self.assertFalse(any(s.items for s in short.sheets))
        self.assertTrue(any(d['code'] == 'drawing.fields_unavailable' for d in short.diagnostics))
        # The source-span audit independently measured 573482 charged values/work units.
        exact = read_drawing_file(path, drawing_limits=DrawingLimits(max_field_values=573482))
        self.assertEqual(len(exact.sheets[0].items),28820)
        self.assertFalse(any(d['code'] == 'drawing.fields_unavailable' for d in exact.diagnostics))

    def test_saved_display_reports_svg_and_viewer_keep_partial_status(self):
        schema = json.loads((ROOT/'schemas/drawing-report-v1.schema.json').read_text())
        for major, name, count, views in CASES:
            with self.subTest(major=major, name=name):
                path = ROOT/f'fixtures/public/drawings/{name}.idw'
                doc = read_drawing(path.read_bytes(),source_id=path.name)
                self.assertEqual(doc.status, 'experimental_partial')
                self.assertFalse(doc.qualified or doc.complete)
                self.assertIsNone(doc.millimeters_per_unit)
                self.assertEqual(doc.current_state, 'unverified')
                sheet, = doc.sheets
                self.assertEqual((len(sheet.items), len(sheet.views)), (count, views))
                self.assertTrue(sheet.omissions)
                self.assertTrue(all(i.source.stream for i in sheet.items))
                hidden_count = {'versions/Toys-R-Us-Rex':17,'versions/mateolikescats':44,
                                'versions/RespiraWorks':21}.get(name)
                if hidden_count is not None:
                    hidden=[i for i in sheet.items if (i.style['layer'] or '').startswith('Hidden')]
                    self.assertEqual(len(hidden),hidden_count)
                    base=.038 if major==26 else .035
                    for item in hidden:
                        self.assertIsNotNone(item.style['dash'])
                        for actual,expected in zip(item.style['dash'],[base*12,base*3]):
                            self.assertAlmostEqual(actual,expected,places=7)
                        self.assertIn('dash_phase_and_fit_unverified',item.style['unresolved'])
                jsonschema.validate(doc.report(), schema)
                with self.assertRaises(DrawingDisplayError):
                    doc.to_svg(sheet_id=sheet.id)
                svg = ET.fromstring(doc.to_svg(sheet_id=sheet.id, allow_partial=True))
                self.assertGreater(len(svg.findall('.//*[@data-item-id]')), 0)
                with self.assertRaises(DrawingDisplayError):
                    doc.render_sheet(sheet_id=sheet.id, allow_partial=True)
                with tempfile.TemporaryDirectory() as directory:
                    scene = build_scene(path, Path(directory), Options())
                    self.assertEqual(scene['drawing']['status'], 'experimental_partial')
                    descriptor=scene['drawing']['sheets'][0]
                    self.assertEqual(descriptor['item_count'], count)
                    self.assertLess(descriptor['bytes'],32*1024*1024)
                    validate_drawing_resources(Path(directory),scene)
                    payload=_unpack_styles(json.loads((Path(directory)/descriptor['resource']).read_bytes()))
                    for actual,item in zip(payload['items'],sheet.items):
                        self.assertEqual(actual,item_dict(item))
                bounded = read_drawing_file(path, drawing_limits=DrawingLimits(max_display_items=count - 1))
                self.assertEqual(bounded.status, 'unavailable')
                self.assertFalse(any(s.items for s in bounded.sheets))
                caches = [i for i in doc.images if i.status == 'decoded_rgba_view_cache_unqualified']
                size = {'versions/mateolikescats': (392, 445),
                        'versions/RespiraWorks-bottom-assembly': (608, 362),
                        'versions/RespiraWorks-filter-panel-assembly': (584, 464)}.get(name)
                self.assertEqual(len(caches), int(size is not None))
                for cache in caches:
                    with Image.open(BytesIO(cache.data)) as png:
                        png.load()
                        self.assertEqual(png.mode, 'RGBA')
                        self.assertEqual(png.size, size)

    def test_unsupported_fonts_stay_explicit_and_filled_annotation_conics_are_present(self):
        template = read_drawing_file(ROOT/'fixtures/public/drawings/iacs/Template_IACS.idw')
        self.assertTrue(any('39 native font entries' in d['message'] for d in template.diagnostics))
        drawing = read_drawing_file(ROOT/'fixtures/public/drawings/versions/RespiraWorks.idw')
        self.assertFalse(any(d['message'] in ('unknown circle suffix','unknown arc suffix') for d in drawing.diagnostics))
        self.assertEqual(sum(bool(i.geometry.get('filled')) for i in drawing.sheets[0].items),14)

    def test_native_hidden_major28_curves_are_omitted_with_provenance(self):
        drawing = read_drawing_file(ROOT/'fixtures/public/drawings/versions/mateolikescats.idw')
        sheet, = drawing.sheets
        # Native Visible=false controls include lines, splines, arcs and projected conics.
        segment = '45f224a8-08f2-4ff8-ab5b-9dde47be9687'
        hidden = {805,1094,1284,1801,1936,1940,1948,1953}
        displayed = {i.record_ordinal for i in sheet.items if i.segment_id == segment}
        self.assertFalse(hidden & displayed)
        omitted = {o['record_ordinal'] for o in sheet.omissions
                   if o['segment_id'] == segment and o['reason'] == 'hidden_by_stored_attribute'}
        self.assertTrue(hidden <= omitted)
        self.assertIn(781,displayed)
        views = {v.name: len(v.item_ids) for v in sheet.views}
        self.assertEqual((views['VIEW6'],views['VIEW9']),(181,186))


if __name__ == '__main__':
    unittest.main()
