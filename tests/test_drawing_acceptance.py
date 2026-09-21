"""Acceptance must not promote missing native evidence or tune reserved families."""
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'scripts'))
from validate_drawing_samples import evaluate, compare_document, POLICY
from inventor_kit import read_drawing_file
sys.path.insert(0, str(ROOT/'tests'))
from test_drawing_oracle import fake_capture
import tempfile
from types import SimpleNamespace
from validate_drawing_control_session import geometry_checks, text_checks
from validate_drawing_annotation_session import line_coverage, raster_checks


class DrawingAcceptance(unittest.TestCase):
    def test_dimension_line_union_rejects_gaps_and_parallel_displacement(self):
        lines = [[[2., 3., 0.], [4., 3., 0.]], [[8., 3., 0.], [4., 3., 0.]]]
        self.assertTrue(line_coverage(lines, [2., 3.], [8., 3.]))
        lines[1][1][0] = 4.1
        self.assertFalse(line_coverage(lines, [2., 3.], [8., 3.]))
        lines[1][1][0] = 4.
        self.assertFalse(line_coverage(lines, [2., 3.01], [8., 3.01]))

    def test_raster_comparison_rejects_misplacement_without_fitting(self):
        from PIL import Image
        import io
        bitmap = Image.new('RGBA', (32, 32), (0, 0, 0, 0))
        for x in range(8, 25): bitmap.putpixel((x, 8), (0, 0, 0, 255))
        png = io.BytesIO(); bitmap.save(png, format='PNG')
        geometry = dict(kind='image', format=3, reference=1, origin=[0., 32., 0.], u=[32., 0., 0.], v=[0., -32., 0.])
        sheet = SimpleNamespace(items=[SimpleNamespace(geometry=geometry)], views=[SimpleNamespace(
            name='Synthetic view', rotation=None, view_type=None, parent_view_id=None)])
        images = {1: SimpleNamespace(data=png.getvalue())}
        capture = lambda value: dict(status='captured', value=value)
        views = [dict(name='Synthetic view', curves=[dict(segments=[dict(visible=capture(True), hidden=capture(False),
                geometry=capture(dict(kind='line', start=[8.,24.], end=[24.,24.])))])])]
        self.assertTrue(all(c['status'] == 'passed' for c in raster_checks(sheet, images, views)))
        geometry['origin'] = [0., 42., 0.]
        self.assertTrue(any(c['status'] == 'failed' for c in raster_checks(sheet, images, views)))

    def test_control_text_checks_detect_lost_rotation_style_and_line_breaks(self):
        import copy
        note = dict(text='AB', formatted_text="<StyleOverride FontSize='0.35' Italic='True'>A</StyleOverride><Br/><StyleOverride FontSize='0.35' Bold='True'>B</StyleOverride>",
                    font='Tahoma', rotation_rad=0.)
        items = [SimpleNamespace(geometry=dict(kind='text', text=text, position=[4., y, 0.],
                    direction=[1., 0., 0.], up=[0., 1., 0.],
                    font=dict(family='Tahoma', height_candidate=.35, flags=flags, weight_candidate=weight)))
                 for text, y, flags, weight in [('A', 9.6, 1, 400), ('B', 9., 0, 700)]]
        self.assertTrue(all(c['status'] == 'passed' for c in text_checks(items, [note])))
        for field, value, check in [('position', [4., 9.6, 0.], 'text_line_breaks_and_order'),
                                    ('direction', [0., 1., 0.], 'text_rotation'),
                                    ('font', dict(family='Tahoma', height_candidate=.35, flags=0, weight_candidate=400), 'text_run_format')]:
            broken = copy.deepcopy(items)
            broken[1].geometry[field] = value
            self.assertTrue(any(c['check'] == check and c['status'] == 'failed' for c in text_checks(broken, [note])))

    def test_circle_comparison_ignores_parameter_phase_but_detects_shape_loss(self):
        import math
        observed = [dict(sheet_basis=[[10., 20.], [10., 22.], [9., 20.]],
                         entities=[dict(kind='circle', center=[0., 0.], radius=2.)])]
        # Independent rotated/skew-scale representation of the same ellipse.
        curve = dict(kind='curve', center=[10., 20., 0.], u=[-2., 0., 0.], v=[0., -4., 0.], start=1., end=1.+math.tau)
        self.assertEqual(geometry_checks([SimpleNamespace(geometry=curve)], observed)[0]['status'], 'passed')
        curve['u'] = [-3., 0., 0.]
        self.assertEqual(geometry_checks([SimpleNamespace(geometry=curve)], observed)[0]['status'], 'failed')

    def test_missing_captures_never_decode_holdouts_or_qualify_inventory(self):
        with patch('inventor_kit.read_drawing_file', side_effect=AssertionError('No unqualified decode')):
            report = evaluate(ROOT/'fixtures/public')
        self.assertEqual(report['input_integrity'], 'passed')
        self.assertFalse(report['qualified'])
        self.assertEqual(report['independent_holdout_families'], [])
        self.assertEqual(sum(r['split'] == 'holdout' for r in report['results']), 2)
        result = subprocess.run([sys.executable, str(ROOT/'scripts/validate_drawing_samples.py')], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertFalse(json.loads(result.stdout)['qualified'])

    def test_unknown_units_and_unimplemented_comparisons_remain_explicit(self):
        with tempfile.TemporaryDirectory() as temporary:
            capture, *_ = fake_capture(Path(temporary))
        doc = read_drawing_file(ROOT/'fixtures/public/SampleBg.idw')
        result = compare_document(doc, capture, json.loads(POLICY.read_text()))
        self.assertEqual(result['status'], 'incomplete')
        self.assertIn('physical_units_unverified', result['gaps'])
        self.assertIn('comparison_not_qualified:dimensions_leaders', result['gaps'])
        self.assertFalse(any(c['check'].endswith('size_mm') for c in result['checks']))
        capture['sheets']['items'] = []
        capture['sheets']['reported_count'] = 0
        self.assertEqual(compare_document(doc, capture, json.loads(POLICY.read_text()))['status'], 'failed')
