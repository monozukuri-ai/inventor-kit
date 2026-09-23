"""Check pinned native major31 line styles and distinguish nominal SVG from PDF fitting.

Requires the private 38-case capture, the built package, and MuPDF's mutool.
The PDF line-fit model is measured separately; it is not implemented by the SVG
renderer and does not qualify curve phase, custom .lin files or other profiles.
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET

from drawing_oracle_contract import identity, read_json
from validate_drawing_linetype_controls import validate, ROOT
from validate_drawing_unit_controls import require, near


def fitted_line_segments(start, end, dash):
    """Candidate native straight-line rule: whole periods, half a dash at each end."""
    if not dash:
        return [[start, end]]
    require(end > start and all(math.isfinite(v) and v > 0 for v in dash), 'Invalid line model')
    require(len(dash) % 2 == 0, 'Expected alternating dash/gap values')
    periods = max(1, round((end-start)/sum(dash)))
    require(periods*len(dash) < 100000, 'Line model work limit')
    scale = (end-start)/(periods*sum(dash))
    lengths = [v*scale for v in dash]
    position = start-lengths[0]/2
    segments = []
    for index in range(periods*len(dash)+1):
        following = position+lengths[index % len(lengths)]
        if index % 2 == 0:
            segments.append([max(position, start), min(following, end)])
        position = following
    return segments


def pdf_segments(pdf, y_mm):
    trace = ET.fromstring(subprocess.run(['mutool', 'draw', '-F', 'trace', str(pdf), '1'],
                         check=True, capture_output=True, text=True).stdout)
    require(len(trace.findall('page')) == 1, 'Expected one PDF page')
    result = []
    for node in trace.iter('stroke_path'):
        if [n.tag for n in node] != ['moveto', 'lineto']:
            continue
        require(node.get('transform') == '1 0 0 -1 0 595', 'Unexpected control PDF transform')
        if node[0].get('y') != node[1].get('y') or abs(float(node[0].get('y'))*25.4/72-y_mm) > .01:
            continue
        result.append(sorted(float(n.get('x'))*25.4/72 for n in node))
    require(bool(result), 'No horizontal control line in PDF')
    return sorted(result)


def measure(root):
    from inventor_kit import read_drawing_file
    acquisition = validate(root)
    pinned = read_json(ROOT/'tests/data/drawing-linetype-controls.json')
    require(acquisition['capture']['sha256'] == pinned['capture_sha256'], 'Capture differs from pinned regression')
    require(acquisition['results'] == pinned['cases'], 'Specimen identities differ')
    capture = read_json(root/'linetypes.native.json')
    patterns = {p['api_enum']: p['nominal_dash_source_units'] for p in pinned['patterns']}
    result = []
    for row in capture['rows']:
        request, native = row['request'], row['sheet']
        dash = patterns[request['pattern']]
        if request['mode'] == 'override':
            dash = [v*request['scale'] for v in dash]
        elif request['by_weight']:
            # Normalize the captured default float32 width to the recorded layer width.
            default_width = 0.03799999877810478
            dash = [v/default_width*request['weight'] for v in dash]
        document = read_drawing_file(root/row['source']['file_name'])
        require(len(document.sheets) == 1, 'Expected one saved sheet')
        sheet = document.sheets[0]
        items = [i for i in sheet.items if i.style['layer'] == 'IK '+row['case']]
        require(len(items) == 2 and {i.geometry['kind'] for i in items} == {'polyline', 'curve'}, 'Missing styled geometry')
        for item in items:
            require(item.style['dash'] is not None and near(item.style['dash'], dash), 'Nominal dash mismatch: '+row['case'])
            require(near([item.style['width']], [request['weight']]), 'Line width mismatch')
            require(('dash_phase_and_fit_unverified' in item.style['unresolved']) == bool(dash), 'Missing phase limitation')
        svg = ET.fromstring(document.to_svg(allow_partial=True))
        for item in items:
            node = next(n for n in svg if n.get('data-item-id') == item.id)
            value = node.get('stroke-dasharray', 'none')
            saved = [] if value == 'none' else [float(v) for v in value.split()]
            require(near(saved, dash), 'SVG nominal dash serialization mismatch')
        start, end = sorted([native['line_start_cm'][0]*10, native['line_end_cm'][0]*10])
        predicted = fitted_line_segments(start, end, [v*10 for v in dash])
        actual = pdf_segments(root/row['pdf']['file_name'], native['line_start_cm'][1]*10)
        require(len(actual) == len(predicted), 'PDF line segment count differs')
        error = max(abs(a-b) for actual_pair, predicted_pair in zip(actual, predicted) for a,b in zip(actual_pair, predicted_pair))
        require(error <= .01, 'PDF line-fit model differs by more than 0.01 mm')
        result.append(dict(case=row['case'], nominal_dash_source_units=dash, styled_items=len(items),
            pdf_line_segments=len(actual), pdf_line_model_max_error_mm=error,
            svg_phase_and_fitting_matches_pdf=not bool(dash)))
    return dict(schema_version=1, status='nominal_styles_and_pdf_line_model_passed',
        capture=acquisition['capture'], qualified_oracle=False, major=31, results=result,
        limitations=['Generated regression controls, not independent holdouts.',
            'SVG uses nominal periods; PDF endpoint fitting and phase are not implemented.',
            'PDF line-model tolerance is 0.01 mm; circle phase and general physical units are unqualified.',
            'No major23 or custom .lin admission.'])


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
    print(result['status'])


if __name__ == '__main__':
    main()
