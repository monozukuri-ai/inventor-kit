"""Validate new line-style captures before using them for decoder development.

This checks acquisition only: it never decodes IDW or assigns dash arrays to
native pattern IDs. Captured API enums do not equal the binary layer IDs.
"""
import argparse
import hashlib
import json
from pathlib import Path

from drawing_oracle_contract import read_json, identity
from validate_drawing_unit_controls import local_file, saved_boundary, near, require

ROOT = Path(__file__).resolve().parents[1]


def cases():
    result = {}
    for pattern in range(37633, 37648):
        for mode in ('layer', 'override'):
            name = f'{mode}-{pattern}'
            result[name] = dict(name=name, pattern=pattern, mode=mode, weight=.025, scale=1., by_weight=False)
    for pattern in (37634, 37638):
        for prefix, mode, weight, scale, by_weight in (
                ('scale', 'override', .025, 2., False), ('weight', 'layer', .05, 1., False),
                ('by-weight', 'layer', .025, 1., True), ('by-weight-double', 'layer', .05, 1., True)):
            name = f'{prefix}-{pattern}'
            result[name] = dict(name=name, pattern=pattern, mode=mode, weight=weight, scale=scale, by_weight=by_weight)
    return result


def validate(root):
    report = read_json(root/'linetypes.native.json')
    require(report['format'] == 'inventor-kit-native-linetype-controls-v1', 'Unknown capture format')
    require(report['qualified_oracle'] is False and report['split'] == 'regression'
            and report['family_id'] == 'inventor-kit-generated-linetype-controls', 'Invalid capture scope')
    script = ROOT/'scripts/create_drawing_linetype_controls.ps1'
    require(report['script']['sha256'] == hashlib.sha256(script.read_bytes()).hexdigest(), 'Collector identity differs')
    expected = cases()
    require(len(report['rows']) == len(expected) and {r['case'] for r in report['rows']} == set(expected),
            'Missing or duplicate line-style cases')
    results = []
    for row in report['rows']:
        require(row['status'] == 'captured', 'Failed native case: '+row['case'])
        request = expected[row['case']]
        require(row['request'] == request, 'Case request differs')
        saved_boundary(row)
        source = local_file(root, row['source'], '.idw')
        pdf = local_file(root, row['pdf'], '.pdf')
        require(source.name == row['case']+'.idw' and pdf.name == row['case']+'.pdf', 'Misbound specimen')
        native = row['sheet']
        require(native['reference_count'] == 0 and native['observation_length_unit'] == 'cm', 'Unexpected dependencies or units')
        require(near([native['width_cm'], native['height_cm']], [29.7, 21.]), 'Unexpected paper dimensions')
        # The requested geometry is constructed in sketch coordinates. Save the
        # API sheet coordinates for comparison; do not assume the sketch basis.
        for key in ('line_start_cm', 'line_end_cm', 'circle_center_cm'):
            require(near(native[key], native[key]) and len(native[key]) == 2, 'Invalid observed point')
        require(near([native['circle_radius_cm']], [3.1]), 'Unexpected circle radius')
        for key in ('line_style', 'circle_style'):
            style = native[key]
            layer = style['layer']
            require(type(style['definition_space']) is int, 'Missing definition space')
            require(layer['name'] == 'IK '+row['case'] and layer['visible'] is True and layer['plot'] is True,
                    'Wrong or hidden control layer')
            require(layer['scale_by_line_weight'] is request['by_weight'], 'Wrong width-dependent scaling')
            require(near([layer['line_weight'], style['line_scale']], [request['weight'], request['scale']]),
                    'Wrong observed width or scale')
            if request['mode'] == 'layer':
                require(style['line_type'] == 37648 and layer['line_type'] == request['pattern'], 'Layer inheritance differs')
            else:
                require(style['line_type'] == request['pattern'] and layer['line_type'] == 37633, 'Entity override differs')
                require(near([style['line_weight']], [request['weight']]), 'Override width differs')
        require(row['pdf_vector_resolution'] == 4800, 'Unexpected requested PDF resolution')
        results.append(dict(case=row['case'], source=identity(source), pdf=identity(pdf)))
    return dict(schema_version=1, status='acquisition_checks_passed', qualified_oracle=False,
        decoder_evaluated=False, pattern_mapping_qualified=False, capture=identity(root/'linetypes.native.json'),
        results=results, limitations=['No native pattern mapping, dash length, phase or PDF fidelity is certified.',
            'Generated regression family only; no independent holdouts.'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new evidence path')
    result = validate(args.input.resolve())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump(result, stream, indent=2, allow_nan=False)
        stream.write('\n')
    print(result['status'])


if __name__ == '__main__':
    main()
