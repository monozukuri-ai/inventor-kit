"""Synthetic capture-corruption tests, not Inventor or linetype qualification."""
import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'scripts'))
import validate_drawing_linetype_controls as controls
import measure_drawing_linetypes as measurement
from drawing_oracle_contract import identity


class LineCapture(unittest.TestCase):
    def test_pdf_line_model_is_separate_from_nominal_svg_and_rejects_bad_lengths(self):
        self.assertEqual(measurement.fitted_line_segments(0, 12, [2, 2]),
                         [[0, 1], [3, 5], [7, 9], [11, 12]])
        self.assertEqual(measurement.fitted_line_segments(0, 12, []), [[0, 12]])
        for dash in ([2, 0], [2, -1], [float('nan'), 2], [2], [1e-8, 1e-8]):
            with self.subTest(dash=dash), self.assertRaises(ValueError):
                measurement.fitted_line_segments(0, 12, dash)

    def test_acquisition_rejects_missing_cases_mutation_and_style_mismatches(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            state = dict(dirty=False, requires_update=False, defer_updates=False, needs_migrating=False,
                         sheet_status=[0], revision='synthetic', file_save_counter=1)
            rows = []
            for name, request in controls.cases().items():
                source, pdf = root/(name+'.idw'), root/(name+'.pdf')
                source.write_bytes(b'synthetic IDW'); pdf.write_bytes(b'synthetic PDF')
                layer = dict(name='IK '+name, visible=True, plot=True, scale_by_line_weight=request['by_weight'],
                             line_type=request['pattern'] if request['mode'] == 'layer' else 37633, line_weight=request['weight'])
                style = dict(layer=layer, definition_space=1, line_scale=request['scale'], line_weight=request['weight'],
                             line_type=request['pattern'] if request['mode'] == 'override' else 37648)
                sheet = dict(width_cm=29.7, height_cm=21, observation_length_unit='cm', reference_count=0,
                    line_start_cm=[2.3,4.1], line_end_cm=[24.7,4.1], circle_center_cm=[15,12], circle_radius_cm=3.1,
                    line_style=style, circle_style=copy.deepcopy(style))
                rows.append(dict(case=name, request=request, status='captured', before=state, after=copy.deepcopy(state),
                    sheet=sheet, sheet_after=copy.deepcopy(sheet), source=identity(source), source_after=identity(source), pdf=identity(pdf),
                    pdf_vector_resolution=4800, capture_scope='freshly_saved_document', reopened_during_capture=False,
                    save_requested_during_capture=False, update_requested_during_capture=False))
            report = dict(format='inventor-kit-native-linetype-controls-v1', qualified_oracle=False, split='regression',
                family_id='inventor-kit-generated-linetype-controls', rows=rows,
                script=dict(sha256=hashlib.sha256((controls.ROOT/'scripts/create_drawing_linetype_controls.ps1').read_bytes()).hexdigest()))
            target = root/'linetypes.native.json'
            with patch('inventor_kit.read_drawing_file', side_effect=AssertionError('Acquisition must not decode inputs')):
                target.write_text(json.dumps(report))
                result = controls.validate(root)
                self.assertEqual(len(result['results']), 38)
                self.assertFalse(result['decoder_evaluated'])
                self.assertFalse(result['pattern_mapping_qualified'])
                for change in ('missing', 'duplicate', 'collector', 'mutated', 'getter', 'scale', 'pattern', 'failed'):
                    broken = copy.deepcopy(report); row = broken['rows'][0]
                    if change == 'missing': broken['rows'].pop()
                    if change == 'duplicate': broken['rows'][-1] = copy.deepcopy(row)
                    if change == 'collector': broken['script']['sha256'] = '0'*64
                    if change == 'mutated': row['after']['dirty'] = True
                    if change == 'getter': row['sheet']['line_style']['definition_space'] = None
                    if change == 'scale': row['sheet']['line_style']['line_scale'] = 2
                    if change == 'pattern': row['sheet']['line_style']['layer']['line_type'] = 37649
                    if change == 'failed': row['status'] = 'failed'
                    row['sheet_after'] = copy.deepcopy(row['sheet'])
                    target.write_text(json.dumps(broken))
                    with self.subTest(change=change), self.assertRaises(ValueError): controls.validate(root)
