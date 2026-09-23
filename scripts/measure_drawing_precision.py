"""Measure the four pinned unit controls against API cm and native PDF vectors.

No fitted scale, rotation or translation is allowed. SVG source coordinates are
converted by the factor corroborated for these snapshots only. A PDF precision
failure is recorded independently of the saved-coordinate gate; this does not
certify a general IDW unit profile, fonts, printing or complete drawing fidelity.
Requires the local pinned capture, pdfinfo and MuPDF's mutool.
"""
import argparse
import json
import math
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET

from drawing_oracle_contract import identity, read_json
from validate_drawing_unit_controls import validate, require

TOLERANCE_MM = .001
PT_TO_MM = 25.4 / 72


def measurement(actual, expected):
    require(len(actual) == len(expected) and len(actual) > 0, 'Invalid measurement dimensions')
    require(all(type(v) in (int, float) and math.isfinite(v) for v in (*actual, *expected)),
            'Non-finite measurement')
    errors = [abs(a - b) for a, b in zip(actual, expected)]
    return dict(actual_mm=list(actual), expected_mm=list(expected), absolute_errors_mm=errors,
                max_error_mm=max(errors), tolerance_mm=TOLERANCE_MM, passed=max(errors) <= TOLERANCE_MM)


def pdf_line(trace, expected):
    """Read the sole straight stroke in a blank generated unit specimen."""
    root = ET.fromstring(trace)
    pages = root.findall('page')
    require(len(pages) == 1, 'Expected a single PDF page')
    page = pages[0]
    box = [float(v) for v in page.attrib['mediabox'].split()]
    require(len(box) == 4 and all(math.isfinite(v) for v in box), 'Invalid PDF media box')
    candidates = []
    for stroke in page.iter('stroke_path'):
        if [child.tag for child in stroke] != ['moveto', 'lineto']:
            continue
        matrix = [float(v) for v in stroke.attrib['transform'].split()]
        require(len(matrix) == 6 and all(math.isfinite(v) for v in matrix), 'Invalid PDF transform')
        a, b, c, d, e, f = matrix
        points = []
        for child in stroke:
            x, y = float(child.attrib['x']), float(child.attrib['y'])
            # MuPDF trace coordinates are top-left; API paper coordinates bottom-left.
            points.extend(((a*x+c*y+e-box[0])*PT_TO_MM, (box[3]-b*x-d*y-f)*PT_TO_MM))
        candidates.append(points)
    require(len(candidates) == 1, 'Expected exactly one straight PDF stroke')
    return measurement(candidates[0], expected), [(box[2]-box[0])*PT_TO_MM, (box[3]-box[1])*PT_TO_MM]


def measure(root):
    from inventor_kit import read_drawing_file
    # Identity, source preservation, save boundary and native API checks precede geometry.
    gate = validate(root)
    capture = read_json(root/'units.native.json')
    rows = []
    for native_row in capture['rows']:
        native = native_row['sheet']
        source, pdf = root/native_row['source']['file_name'], root/native_row['pdf']['file_name']
        doc = read_drawing_file(source)
        sheet = doc.sheets[0]
        line = next(i for i in sheet.items if i.geometry['kind'] == 'polyline' and len(i.geometry['points']) == 2)
        circle = next(i for i in sheet.items if i.geometry['kind'] == 'curve')
        expected_line = [v*10 for key in ('line_start_cm', 'line_end_cm') for v in native[key]]
        expected_paper = [native['width_cm']*10, native['height_cm']*10]
        saved = dict(paper=measurement([v*10 for v in sheet.size_in_source_units], expected_paper),
            line=measurement([v*10 for p in line.geometry['points'] for v in p[:2]], expected_line),
            circle=measurement([v*10 for v in circle.geometry['center'][:2]] +
                [math.hypot(*circle.geometry[k])*10 for k in ('u', 'v')],
                [v*10 for v in native['circle_center_cm']] + [native['circle_radius_cm']*10]*2))
        svg = ET.fromstring(doc.to_svg(allow_partial=True))
        node = next(n for n in svg if n.get('data-item-id') == line.id)
        values = [float(v) for v in re.split(r'[ ,]+', node.attrib['points'])]
        require(len(values) == 4, 'Unexpected SVG line serialization')
        svg_line = measurement([values[0]*10, (sheet.size_in_source_units[1]-values[1])*10,
                                values[2]*10, (sheet.size_in_source_units[1]-values[3])*10], expected_line)
        trace = subprocess.run(['mutool', 'draw', '-F', 'trace', str(pdf), '1'], check=True,
                               capture_output=True, text=True).stdout
        pdf_geometry, pdf_paper = pdf_line(trace, expected_line)
        require(all(m['passed'] for m in saved.values()) and svg_line['passed'],
                'Saved/API or SVG precision regression: '+native_row['case'])
        rows.append(dict(case=native_row['case'], source=identity(source), pdf=identity(pdf),
            millimeters_per_source_unit=10, scale_scope='this_pinned_snapshot_only',
            saved_against_api=saved, svg_line_against_api=svg_line,
            pdf_against_api=dict(line=pdf_geometry, paper=measurement(pdf_paper, expected_paper))))
    return dict(schema_version=1, status='measured', saved_geometry_status='passed',
        pdf_geometry_status='passed' if all(r['pdf_against_api']['line']['passed'] for r in rows) else 'outside_tolerance',
        qualified_oracle=False, general_units_qualified=False, tolerance_mm=TOLERANCE_MM,
        unit_capture=gate['capture'], results=rows,
        limitations=['Four regression snapshots only; reserved holdouts were not decoded.',
            'PDF rounding is measured separately and is not a 0.001 mm reference.',
            'SVG checks cover line coordinate serialization, not printer or browser physical calibration.'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new evidence path')
    result = measure(args.input.resolve())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump(result, stream, indent=2, allow_nan=False)
        stream.write('\n')
    print(json.dumps(dict(saved_geometry=result['saved_geometry_status'], pdf_geometry=result['pdf_geometry_status'])))


if __name__ == '__main__':
    main()
