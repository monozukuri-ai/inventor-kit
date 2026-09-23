"""Pinned real-file regression for added profiles, without native accuracy claims."""
from io import BytesIO
import json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET

import jsonschema
from PIL import Image
from inventor_kit import read_drawing_file, DrawingDisplayError, DrawingLimits
from inventor_kit.viewer.scene import Options, build_scene

ROOT = Path(__file__).resolve().parents[1]
CASES = [
    (24, 'iacs/Template_IACS', 70, 0),
    (29, 'versions/Toys-R-Us-Rex', 166, 3),
    (28, 'versions/mateolikescats', 1816, 5),
    (26, 'versions/RespiraWorks', 529, 3),
    (26, 'versions/starliliko', 90, 0),
    (26, 'versions/RespiraWorks-bottom-assembly', 528, 3),
    (26, 'versions/RespiraWorks-filter-panel-assembly', 9475, 4),
]


class DrawingVersions(unittest.TestCase):
    def test_saved_display_reports_svg_and_viewer_keep_partial_status(self):
        schema = json.loads((ROOT/'schemas/drawing-report-v1.schema.json').read_text())
        for major, name, count, views in CASES:
            with self.subTest(major=major, name=name):
                path = ROOT/f'fixtures/public/drawings/{name}.idw'
                doc = read_drawing_file(path)
                self.assertEqual(doc.status, 'experimental_partial')
                self.assertFalse(doc.qualified or doc.complete)
                self.assertIsNone(doc.millimeters_per_unit)
                self.assertEqual(doc.current_state, 'unverified')
                sheet, = doc.sheets
                self.assertEqual((len(sheet.items), len(sheet.views)), (count, views))
                self.assertTrue(sheet.omissions)
                self.assertTrue(all(i.source.stream for i in sheet.items))
                jsonschema.validate(doc.report(), schema)
                with self.assertRaises(DrawingDisplayError):
                    doc.to_svg(sheet_id=sheet.id)
                svg = ET.fromstring(doc.to_svg(sheet_id=sheet.id, allow_partial=True))
                self.assertGreater(len(svg.findall('.//*[@data-item-id]')), 0)
                with self.assertRaises(DrawingDisplayError):
                    doc.render_sheet(sheet_id=sheet.id, allow_partial=True)
                with tempfile.TemporaryDirectory() as directory:
                    scene = build_scene(path, Path(directory), Options())
                    if name == 'versions/RespiraWorks-filter-panel-assembly':
                        self.assertIsNone(scene['drawing'])
                        self.assertTrue(any(d['message'] == 'Drawing sheet JSON byte limit exceeded'
                                            for d in scene['diagnostics']))
                    else:
                        self.assertEqual(scene['drawing']['status'], 'experimental_partial')
                        self.assertEqual(scene['drawing']['sheets'][0]['item_count'], count)
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

    def test_unsupported_fonts_and_circle_variants_stay_explicit(self):
        template = read_drawing_file(ROOT/'fixtures/public/drawings/iacs/Template_IACS.idw')
        self.assertTrue(any('39 native font entries' in d['message'] for d in template.diagnostics))
        drawing = read_drawing_file(ROOT/'fixtures/public/drawings/versions/RespiraWorks.idw')
        self.assertTrue(any(d['message'] == 'unknown circle suffix' for d in drawing.diagnostics))


if __name__ == '__main__':
    unittest.main()
