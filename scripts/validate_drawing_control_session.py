"""Compare the generated native control session without qualifying general IDW units.

Run after recovering the ZIP produced by create_drawing_controls.ps1. The original
JSON/IDW/PDF bytes are never rewritten. A failed comparison is retained in the report.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import xml.etree.ElementTree as ET

from drawing_oracle_contract import identity, read_json

TOLERANCE = 1e-6  # API/raw numeric correlation only; not a paper/mm qualification.
MANIFEST = Path(__file__).resolve().parents[1]/'tests/data/drawing-additional-controls.json'


def near(actual, expected):
    if isinstance(expected, (list, tuple)):
        return len(actual) == len(expected) and all(near(a, e) for a, e in zip(actual, expected))
    return math.isfinite(actual) and abs(actual - expected) <= TOLERANCE


def on_sheet(point, basis):
    origin, x, y = basis
    return [origin[i] + point[0] * (x[i] - origin[i]) + point[1] * (y[i] - origin[i]) for i in range(2)] + [0.]


def geometry_checks(items, sketches):
    results = []
    lines = [i.geometry for i in items if i.geometry['kind'] == 'polyline' and len(i.geometry['points']) == 2]
    curves = [i.geometry for i in items if i.geometry['kind'] == 'curve']
    for sketch in sketches:
        basis = sketch['sheet_basis']
        for entity in sketch['entities']:
            kind = entity['kind']
            if kind == 'line':
                endpoints = [on_sheet(entity[k], basis) for k in ('start', 'end')]
                matches = [g for g in lines if near(g['points'], endpoints) or near(g['points'][::-1], endpoints)]
            else:
                center = on_sheet(entity['center'], basis)
                radius = entity['radius']
                def point(angle):
                    return on_sheet([entity['center'][0] + radius * math.cos(angle),
                                     entity['center'][1] + radius * math.sin(angle)], basis)
                angles = ([0., math.pi/2, math.pi, math.pi*1.5, math.tau] if kind == 'circle' else
                          [entity['start_angle'] + entity['sweep_angle'] * i / 4 for i in range(5)])
                expected = [point(a) for a in angles]
                matches = []
                for g in curves:
                    if kind == 'circle':
                        # A circle has no API-observed start phase. Compare its
                        # affine ellipse metric, not an arbitrary parameter axis.
                        axes = [[point(a)[j] - center[j] for j in range(3)] for a in (0., math.pi/2)]
                        actual_metric = [[g['u'][i]*g['u'][j] + g['v'][i]*g['v'][j] for j in range(3)] for i in range(3)]
                        expected_metric = [[sum(v[i]*v[j] for v in axes) for j in range(3)] for i in range(3)]
                        if near(g['center'], center) and near(actual_metric, expected_metric) and near(g['end']-g['start'], math.tau):
                            matches.append(g)
                        continue
                    actual = [[g['center'][j] + g['u'][j] * math.cos(a) + g['v'][j] * math.sin(a) for j in range(3)]
                              for a in [g['start'] + (g['end'] - g['start']) * i / 4 for i in range(5)]]
                    if near(g['center'], center) and (near(actual, expected) or near(actual[::-1], expected)):
                        matches.append(g)
            results.append(dict(check=kind + '_sheet_coordinates', status='passed' if len(matches) == 1 else 'failed', matches=len(matches)))
    return results


def text_checks(items, notes):
    """Compare this single-note control layout, preserving line breaks separately.

    Note.Position is a box anchor, not the stored text baseline. No baseline or
    width is inferred from that anchor. Unsupported formatting fails explicitly.
    """
    geometries = [i.geometry for i in items if i.geometry['kind'] == 'text']
    actual = ''.join(g['text'] for g in geometries)
    expected = ''.join(n['text'] for n in notes)
    checks = [dict(check='text_content', actual=actual, expected=expected,
                   status='passed' if actual == expected else 'failed')]
    if not notes:
        return checks
    if len(notes) != 1:
        raise ValueError('Control comparison supports one note per sheet')
    note = notes[0]
    root = ET.fromstring('<Note>'+note['formatted_text']+'</Note>')
    runs, lines = [], ['']
    for node in root:
        if node.tag == 'Br' and not node.attrib and not node.text:
            lines.append('')
        elif node.tag == 'StyleOverride' and len(node) == 0 and node.text:
            if set(node.attrib) - {'Font', 'FontSize', 'Bold', 'Italic'}:
                raise ValueError('Unsupported native control formatting')
            runs.append((node.text, node.attrib))
            lines[-1] += node.text
        else:
            raise ValueError('Unsupported native control text structure')
        if node.tail:
            raise ValueError('Unexpected native control text tail')
    if root.text:
        raise ValueError('Unexpected native control text prefix')
    matched = len(runs) == len(geometries) and all(text == g['text'] for (text, _), g in zip(runs, geometries))
    checks.append(dict(check='text_runs', status='passed' if matched else 'failed'))
    if matched:
        for index, ((_, attributes), g) in enumerate(zip(runs, geometries)):
            font = g['font']
            match = font is not None and font['family'] == attributes.get('Font', note['font'])
            if 'FontSize' in attributes:
                match = match and near(font['height_candidate'], float(attributes['FontSize']))
            if 'Bold' in attributes:
                match = match and font['weight_candidate'] == (700 if attributes['Bold'] == 'True' else 400)
            if 'Italic' in attributes:
                match = match and bool(font['flags'] & 1) == (attributes['Italic'] == 'True')
            checks.append(dict(check='text_run_format', run=index, status='passed' if match else 'failed'))
    angle = note['rotation_rad']
    direction, up = [math.cos(angle), math.sin(angle), 0.], [-math.sin(angle), math.cos(angle), 0.]
    rotation = bool(geometries) and all(near(g['direction'], direction) and near(g['up'], up) for g in geometries)
    checks.append(dict(check='text_rotation', status='passed' if rotation else 'failed'))
    # Group consecutive runs by baseline projection. Line breaks must exist in
    # the geometry even though GeneralNote.Text omits the <Br/> markers.
    actual_lines, baseline, previous_x = [], None, None
    ordered = True
    for g in geometries:
        y = sum(a*b for a, b in zip(g['position'], up))
        x = sum(a*b for a, b in zip(g['position'], direction))
        if baseline is None or not near(y, baseline):
            ordered = ordered and (baseline is None or y < baseline)
            actual_lines.append(g['text']); baseline = y
        else:
            ordered = ordered and (previous_x is None or x > previous_x)
            actual_lines[-1] += g['text']
        previous_x = x
    checks.append(dict(check='text_line_breaks_and_order', actual=actual_lines, expected=lines,
                       status='passed' if ordered and actual_lines == lines else 'failed'))
    return checks


def validate(root):
    from inventor_kit import read_drawing_file
    root = Path(root).resolve()
    manifest = read_json(MANIFEST)
    for recorded in manifest['files']:
        path = root/recorded['file_name']
        if path.is_symlink() or identity(path) != recorded:
            raise ValueError('Pinned control identity mismatch: ' + recorded['file_name'])
    capture = read_json(root/'controls.native.json')
    if capture['format'] != 'inventor-kit-native-controls-v1' or len(capture['rows']) != 14 or capture['qualified_oracle'] is not False:
        raise ValueError('Unexpected native control session')
    results, seen = [], set()
    for row in capture['rows']:
        for recorded in (row['source'], row['pdf']):
            if recorded is None:
                raise ValueError('Native PDF is required for control comparison')
            name = recorded['file']
            if Path(name).name != name or name in seen or '\\' in name:
                raise ValueError('Invalid or duplicate control filename')
            seen.add(name)
            path = root/name
            if path.is_symlink():
                raise ValueError('Control must not be a symbolic link')
            actual = identity(path)
            if (actual['sha256'], actual['bytes']) != (recorded['sha256'], recorded['bytes']):
                raise ValueError('Native control identity mismatch: ' + name)
        source = root/row['source']['file']
        doc = read_drawing_file(source)
        checks = [dict(check='sheet_count', status='passed' if len(doc.sheets) == 1 else 'failed')]
        if len(doc.sheets) == 1:
            sheet = doc.sheets[0]
            checks.append(dict(check='raw_size_api_cm_correlation', status='passed' if sheet.size_in_source_units is not None and
                               near(sheet.size_in_source_units, [row['sheet']['width_cm'], row['sheet']['height_cm']]) else 'failed'))
            checks += geometry_checks(sheet.items, row['sketches'])
            checks += text_checks(sheet.items, row['notes'])
        results.append(dict(file=source.name, source_sha256=doc.source_sha256, status=doc.status,
            checks=checks, native_before=row['before'], native_after=row['after'],
            automatic_update=row['automatic_update'], diagnostics=[dict(d) for d in doc.diagnostics if d['source'] is None],
            unverified=['general_physical_units', 'text_baseline_width_alignment', 'font_substitution', 'full_image_comparison']))
    return dict(schema_version=1, status='bounded_checks_passed' if all(c['status'] == 'passed' for r in results for c in r['checks']) else 'failed',
                qualified=False, holdouts_used=False, provider=capture['version'], font_files=len(capture['font_files']),
                capture_sha256=hashlib.sha256((root/'controls.native.json').read_bytes()).hexdigest(),
                manifest_sha256=hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),
                scope='one_generated_regression_family_not_independent_holdouts', results=results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Output must be a new report path')
    result = validate(args.input)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2, ensure_ascii=False, allow_nan=False)+'\n', encoding='utf-8')
    print(json.dumps({k:v for k,v in result.items() if k != 'results'}))
    raise SystemExit(0 if result['status'] == 'bounded_checks_passed' else 1)


if __name__ == '__main__':
    main()
