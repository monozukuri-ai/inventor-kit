"""Validate saved-boundary unit controls; never qualify unrelated IDW profiles.

The PowerShell generator creates new files and starts acquisition after SaveAs.
Original control bytes, API observations and PDF media boxes are checked offline.
This evidence is separate from full drawing fidelity and independent holdouts.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import subprocess

from drawing_oracle_contract import identity, read_json

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / 'tests/data/drawing-unit-controls.json'
CASES = {'unit-mm': 11269, 'unit-inch': 11272, 'unit-cm': 11268, 'custom-paper': 11269}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def near(actual, expected, tolerance=1e-7):
    return (len(actual) == len(expected) and all(type(a) in (int, float) and math.isfinite(a)
            and abs(a-b) <= tolerance for a, b in zip(actual, expected)))


def saved_boundary(row, *, sheet_count=1):
    """State fields corroborate the explicit save boundary, not opening-time state."""
    require(row['capture_scope'] == 'freshly_saved_document', 'Missing fresh save boundary')
    for key in ('reopened_during_capture', 'save_requested_during_capture', 'update_requested_during_capture'):
        require(row[key] is False, 'Acquisition changed or reopened the document: ' + key)
    for label in ('before', 'after'):
        state = row[label]
        for key in ('dirty', 'requires_update', 'defer_updates', 'needs_migrating'):
            require(state[key] is False, f'{label}: unverified saved state: {key}')
        require(type(sheet_count) is int and sheet_count > 0 and isinstance(state['sheet_status'], list)
                and len(state['sheet_status']) == sheet_count
                and all(type(v) is int and v == 0 for v in state['sheet_status']),
                label + ': sheet is not settled')
        require(type(state['file_save_counter']) is int and state['file_save_counter'] > 0,
                label + ': missing save counter')
        require(isinstance(state['revision'], str) and bool(state['revision'].strip()),
                label + ': missing database revision')
    require(row['before'] == row['after'], 'Document state changed during acquisition')
    require(row['sheet'] == row['sheet_after'], 'API values changed during PDF export')
    require(row['source'] == row['source_after'], 'IDW bytes changed during acquisition')


def local_file(root, record, suffix):
    name = record['file_name']
    require(isinstance(name, str) and Path(name).name == name and '\\' not in name,
            'Unsafe control filename')
    path = root / name
    require(path.suffix.lower() == suffix and not path.is_symlink() and path.is_file(),
            'Missing or invalid control file')
    require(identity(path) == record, 'Control hash/size mismatch: ' + name)
    return path


def validate(root):
    from inventor_kit import read_drawing_file
    root = Path(root).resolve()
    manifest = read_json(MANIFEST)
    require(manifest['split'] == 'regression' and manifest['independent_holdout'] is False,
            'Unit controls must not qualify as independent holdouts')
    for record in manifest['files']:
        local_file(root, record, Path(record['file_name']).suffix)
    report = read_json(root / 'units.native.json')
    require(report['format'] == 'inventor-kit-native-unit-controls-v1', 'Unknown unit evidence format')
    require(report['split'] == 'regression' and report['family_id'] == 'inventor-kit-generated-controls-2027',
            'Unit controls must remain regression inputs')
    require(report['qualified_oracle'] is False, 'Generator cannot certify its own oracle')
    require(report['family_id'] == manifest['family'] and report['script'] == manifest['script'],
            'Pinned collector or family differs')
    script = ROOT / 'scripts/create_drawing_unit_controls.ps1'
    require(report['script']['sha256'] == hashlib.sha256(script.read_bytes()).hexdigest(),
            'Collector identity differs; review its save-boundary behavior first')
    rows = report['rows']
    require(len(rows) == len(CASES) and {r['case'] for r in rows} == set(CASES), 'Missing/repeated unit control')
    results = []
    for row in rows:
        require(row['status'] == 'captured', 'Native unit control failed: ' + row['case'])
        saved_boundary(row)
        source = local_file(root, row['source'], '.idw')
        pdf = local_file(root, row['pdf'], '.pdf')
        native = row['sheet']
        require(native['observation_length_unit'] == 'cm' and native['display_length_units'] == CASES[row['case']],
                'Unexpected API/display units')
        require(native['reference_count'] == 0, 'Unit specimen must not depend on a model')
        doc = read_drawing_file(source)
        require(len(doc.sheets) == 1 and doc.sheets[0].status != 'unavailable', 'Unresolved unit-control sheet')
        sheet = doc.sheets[0]
        require(native['name'] == sheet.name + ':1', 'Control sheet name differs')
        paper = [native['width_cm'], native['height_cm']]
        expected_paper = [32.1, 17.3] if row['case'] == 'custom-paper' else [29.7, 21.]
        require(near(paper, expected_paper), 'Native paper differs from the requested control size')
        require(near(sheet.size_in_source_units, paper), 'Saved paper size differs from API cm')
        lines = [i.geometry['points'] for i in sheet.items
                 if i.geometry['kind'] == 'polyline' and len(i.geometry['points']) == 2]
        require(sum(near(p[0][:2], native['line_start_cm']) and near(p[1][:2], native['line_end_cm'])
                    for p in lines) == 1, 'Saved line differs from API cm')
        circles = [i.geometry for i in sheet.items if i.geometry['kind'] == 'curve']
        require(len(circles) == 1 and near(circles[0]['center'][:2], native['circle_center_cm']),
                'Saved circle center differs from API cm')
        require(near([math.hypot(*circles[0][k]) for k in ('u', 'v')], [native['circle_radius_cm']] * 2),
                'Saved circle radius differs from API cm')
        texts = [i.geometry['text'] for i in sheet.items if i.geometry['kind'] == 'text']
        require(texts == [native['text']], 'Saved text differs from API')
        info = subprocess.run(['pdfinfo', str(pdf)], check=True, capture_output=True, text=True).stdout
        size = re.search(r'^Page size:\s+([\d.]+) x ([\d.]+) pts', info, re.M)
        pages = re.search(r'^Pages:\s+(\d+)', info, re.M)
        require(size is not None and pages is not None and int(pages[1]) == 1, 'Invalid PDF page inventory')
        pdf_mm = [float(size[i]) * 25.4 / 72 for i in (1, 2)]
        require(near(pdf_mm, [v * 10 for v in paper], 25.4 / 72), 'PDF paper differs by more than one point')
        results.append(dict(case=row['case'], source=row['source'], pdf=row['pdf'],
                            save_boundary='verified_for_recorded_snapshot', display_length_units=CASES[row['case']],
                            source_coordinates_match_api_cm=True, millimeters_per_source_unit=10,
                            paper_size_mm=[v * 10 for v in paper], pdf_size_mm=pdf_mm))
    return dict(schema_version=1, status='saved_snapshot_checks_passed', qualified_oracle=False,
                family_id=report['family_id'], independent_holdout=False, results=results,
                capture=identity(root / 'units.native.json'),
                manifest_sha256=hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),
                limitations=['Evidence applies to these four snapshots, not all major31 files.',
                             'Not the full drawing-oracle schema or a text/style/clip fidelity gate.'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new report path; existing evidence is never overwritten')
    report = validate(args.input)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8') as output:
        json.dump(report, output, indent=2, ensure_ascii=False, allow_nan=False)
        output.write('\n')
    print(report['status'])


if __name__ == '__main__':
    main()
