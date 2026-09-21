"""Compare the pinned, locally supplied 2027.1 controls; never certify an oracle.

Requires the validation extra, Cargo, and Poppler tools. Binary controls are not
distributed. Exit 0 means the bounded observations/integration checks passed,
not that all annotations, physical units or current state are qualified.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET

from drawing_oracle_contract import read_json
from validate_drawing_inventory import verify_sources

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / 'tests/data/drawing-native-controls.json'


def require(value, message):
    if not value:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(*args):
    return subprocess.run([str(a) for a in args], cwd=ROOT, check=True,
                          capture_output=True, text=True, encoding='utf-8',
                          env={**os.environ, 'LC_ALL': 'C'}).stdout


def fields(observation):
    return {f['name']: f['value']['raw'] for f in observation['fields']}


def observations(inventory, role):
    return [(s, o) for s in inventory['segments'] for o in s['observations']
            if o['proposed_role'] == role]


def native(root, name):
    # The original PowerShell 5.1 captures include a UTF-8 BOM. Keep originals.
    return json.loads((root / name).read_text(encoding='utf-8-sig'))


def verify_inputs(root, manifest):
    expected = {r['file'] for r in manifest['files']}
    require({p.name for p in root.iterdir()} == expected, 'Control file list differs')
    for row in manifest['files']:
        path = root / row['file']
        require(path.parent == root and not path.is_symlink() and path.is_file(),
                'Control must be a regular local file')
        require(path.stat().st_size == row['bytes'] and digest(path) == row['sha256'],
                f"Control identity mismatch: {row['file']}")
    sheets = native(root, 'sheets.native.json')
    exports = native(root, 'exports.native.json')
    require(len(sheets) == len(exports) == 9, 'Expected nine native documents/exports')
    for row, exported in zip(sheets, exports):
        require(row['file'] == exported['file'], 'Capture/export order differs')
        path = root / row['file']
        require(digest(path) == row['sha256'].lower() == exported['source_sha256'].lower(),
                'Capture IDW hash mismatch')
        require(path.stat().st_size == row['bytes'], 'Capture IDW size mismatch')
        require(digest(root / exported['pdf']) == exported['pdf_sha256'].lower(),
                'Capture PDF hash mismatch')
    return sheets, exports


def pdf_report(root, output, row):
    path = root / Path(row['file']).with_suffix('.pdf')
    count = len(row['sheets'])
    info = run('pdfinfo', '-f', 1, '-l', count, path)
    pages = int(re.search(r'^Pages:\s+(\d+)', info, re.M)[1])
    sizes = [(float(w), float(h)) for w, h in re.findall(
        r'^Page\s+\d+ size:\s+([\d.]+) x ([\d.]+) pts', info, re.M)]
    require(pages == count == len(sizes), 'PDF page count mismatch')
    deltas = []
    for size, sheet in zip(sizes, row['sheets']):
        expected = [sheet['width_cm'], sheet['height_cm']]
        delta = [actual * 2.54 / 72 - target for actual, target in zip(size, expected)]
        # Inventor PDFs round the paper box to integer points; at most one point.
        require(all(abs(v) <= 2.54 / 72 for v in delta), 'PDF paper size differs by >1pt')
        deltas.append(delta)
    fonts = run('pdffonts', path)
    extracted = run('pdftotext', '-layout', path, '-')
    (output / f'{path.stem}.pdfinfo.txt').write_text(info)
    (output / f'{path.stem}.pdffonts.txt').write_text(fonts)
    (output / f'{path.stem}.pdftotext.txt').write_text(extracted, encoding='utf-8')
    run('pdftoppm', '-r', 150, '-png', path, output / path.stem)
    images = sorted(output.glob(f'{path.stem}-*.png'))
    require(len(images) == count, 'PDF raster page count mismatch')
    return dict(pages=pages, sizes_points=sizes, size_delta_cm=deltas,
                text=extracted.strip(), fonts=fonts,
                rasters=[dict(file=p.name, sha256=digest(p)) for p in images])


def compare(root, output, inventory, row, exported):
    from inventor_kit import read_drawing_file
    from inventor_kit.viewer.scene import Options, build_scene, write_scene
    source = root / row['file']
    count = verify_sources(inventory, source)
    require(all(s['registry']['major'] == 31 and s['status'] == 'framed'
                for s in inventory['segments']), 'Unframed native major31 segment')
    require(not inventory['diagnostics'] and not any(s['diagnostics'] for s in inventory['segments']),
            'Unexpected native inventory diagnostic')
    doc = read_drawing_file(source)
    require(len(doc.sheets) == len(row['sheets']), 'Stored sheet count differs')
    for i, (sheet, observed) in enumerate(zip(doc.sheets, row['sheets']), 1):
        # Record the distinction: the native API appends :<ordinal> to this family.
        require(observed['name'] == f'{sheet.name}:{i}', 'Sheet order/name differs')
        require(sheet.size_in_source_units == (observed['width_cm'], observed['height_cm']),
                'Resolved sheet size/API pairing differs')
        require(sheet.status == 'experimental_partial' and not sheet.diagnostics,
                'Native sheet display is unresolved')
    raw_extents = [fields(o)['origin_and_extent_candidate'][2:]
                   for _, o in observations(inventory, 'sheet_space_candidate')]
    api_extents = [[s['width_cm'], s['height_cm']] for s in row['sheets']]
    require(sorted(raw_extents) == sorted(api_extents), 'Raw/API extent multiset differs')
    require(not doc.qualified and not doc.complete and doc.current_state == 'unverified',
            'Native controls must not certify drawing completeness/state')
    require(doc.units == 'source_units_unverified' and doc.millimeters_per_unit is None,
            'Do not promote field correlations to a general unit contract')
    target = output / source.stem
    target.mkdir()
    scene = build_scene(source, target, Options(experimental_drawing=True))
    require(scene['drawing']['status'] == doc.status, 'Viewer/API status differs')
    require([s['name'] for s in scene['drawing']['sheets']] == [s.name for s in doc.sheets],
            'Viewer/API sheet list differs')
    require([s['item_count'] for s in scene['drawing']['sheets']] == [len(s.items) for s in doc.sheets],
            'Viewer/API element counts differ')
    write_scene(target, scene)
    expected_items = {'line': 2, 'text-ascii': 2, 'text-japanese': 5}.get(source.stem, 1)
    require(all(len(s.items) == expected_items for s in doc.sheets), 'Native display element count differs')
    require(inventory.get('revisions'), 'Expected bounded revision identities')
    result = dict(file=row['file'], sha256=doc.source_sha256, framed_records=count,
                  raw_extents=raw_extents, api_extents_cm=api_extents,
                  api_display_length_units=row['display_length_units'],
                  stored_names=[s.name for s in doc.sheets], native_names=[s['name'] for s in row['sheets']],
                  display_status=doc.status,
                  sheets=[dict(name=s.name, status=s.status, size=s.size_in_source_units,
                               items=len(s.items), omissions=len(s.omissions), diagnostics=s.diagnostics) for s in doc.sheets],
                  display_diagnostics=[dict(d) for d in doc.diagnostics if d['source'] is None],
                  native_export_state=exported, pdf=pdf_report(root, output, row))
    if source.stem == 'line':
        observed = native(root, 'line.native.json')
        lines = observations(inventory, 'stored_line_candidate')
        require(len(lines) == 1, 'Expected one stored line')
        coordinates = fields(lines[0][1])['line_endpoints']
        expected = [*observed['sheet_start_cm'], 0, *observed['sheet_end_cm'], 0]
        require(coordinates == expected, 'Raw/API line coordinates differ')
        displayed = [i.geometry for i in doc.sheets[0].items
                     if i.geometry['kind'] == 'polyline' and len(i.geometry['points']) == 2]
        require(len(displayed) == 1 and list(sum(displayed[0]['points'], ())) == expected,
                'Placed scene line/API coordinates differ')
        result['line'] = dict(raw=coordinates, api_cm=expected, source=lines[0][1]['source'])
        svg = output / 'line-native.svg'
        run('pdftocairo', '-svg', source.with_suffix('.pdf'), svg)
        paths = ET.parse(svg).getroot().findall('{http://www.w3.org/2000/svg}path')
        result['line']['pdf_paths'] = [p.attrib for p in paths]
    if source.stem.startswith('text-'):
        observed = native(root, source.stem + '.native.json')
        texts = observations(inventory, 'stored_text_candidate')
        raw = [dict(**fields(o), source=o['source']) for _, o in texts]
        require(''.join(t['text'] for t in raw) == observed['text'], 'Raw/API text differs')
        displayed = [i.geometry for i in doc.sheets[0].items if i.geometry['kind'] == 'text']
        require(''.join(t['text'] for t in displayed) == observed['text'], 'Scene/API text differs')
        for stored, placed in zip(raw, displayed):
            require(list(placed['position']) == stored['position_and_direction_candidate'][:3],
                    'SM paper-space text position changed')
        require(all(s['registry']['kind'] == 'DlSheetSmSegmentType' for s, _ in texts),
                'Expected text in the SM segment')
        fonts = [fields(o) for _, o in observations(inventory, 'font_table_candidate')]
        require(len(fonts) == 1 and math.isclose(fonts[0]['font_size_parameters'][0], .35, abs_tol=1e-7),
                'Stored display font size does not match inline override')
        result['text'] = dict(runs=raw, native=observed, stored_fonts=fonts,
                             placement_interpretation='baseline_candidate_not_API_note_anchor')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--archive', type=Path)
    parser.add_argument('--output', type=Path, required=True, help='New evidence directory')
    args = parser.parse_args()
    root = args.input.resolve()
    manifest = read_json(MANIFEST)
    sheets, exports = verify_inputs(root, manifest)
    if args.archive:
        require(args.archive.stat().st_size == manifest['archive']['bytes']
                and digest(args.archive) == manifest['archive']['sha256'], 'Archive identity mismatch')
    args.output.mkdir(parents=True, exist_ok=False)
    run('cargo', 'build', '--locked', '-p', 'inventor-core', '--example', 'inspect_drawing')
    reader = ROOT / 'target/debug/examples/inspect_drawing'
    if os.name == 'nt':
        reader = reader.with_suffix('.exe')
    parsed = run(reader, *(root / r['file'] for r in sheets))
    (args.output / 'inventory.jsonl').write_text(parsed, encoding='utf-8')
    inventories = [json.loads(line) for line in parsed.splitlines()]
    require(len(inventories) == len(sheets), 'Incomplete inventory output')
    rows = [compare(root, args.output, inv, row, exported)
            for inv, row, exported in zip(inventories, sheets, exports)]
    # Recheck the immutable native inputs after all parsing and PDF conversions.
    verify_inputs(root, manifest)
    report = dict(status='bounded_checks_passed', scope=manifest['scope'],
                  family=manifest['family'], holdouts_used=False, qualified_oracle=False,
                  archive_verified=bool(args.archive), input_files_verified=len(manifest['files']),
                  raster_dpi=150, poppler_version=subprocess.run(['pdftoppm', '-v'],
                      capture_output=True, text=True, check=True).stderr.strip(),
                  validator_sha256=digest(Path(__file__)), reader_sha256=digest(reader),
                  rendering_comparison='incomplete',
                  fixtures=rows)
    (args.output / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: v for k, v in report.items() if k != 'fixtures'}, ensure_ascii=False))


if __name__ == '__main__':
    main()
