"""Check migrated holdout acquisition without decoding or tuning reserved IDWs.

This gate verifies pinned original bytes, saved-copy identity and API/PDF capture
boundaries. It does not substitute for the full drawing oracle or fidelity gate.
"""
import argparse
import hashlib
import json
from pathlib import Path

from drawing_oracle_contract import identity, read_json, walk
from validate_drawing_unit_controls import require, saved_boundary, local_file

ROOT = Path(__file__).resolve().parents[1]
FAMILIES = {'forge-rim', 'lfrum-vise'}


def validate(root):
    root = Path(root).resolve()
    report = read_json(root / 'holdouts.native.json')
    require(report['format'] == 'inventor-kit-migrated-holdouts-v1', 'Unknown capture format')
    require(report['qualified_oracle'] is False, 'Acquisition cannot certify itself')
    script = ROOT / 'scripts/capture_drawing_holdouts.ps1'
    require(report['script']['sha256'] == hashlib.sha256(script.read_bytes()).hexdigest(), 'Collector identity changed')
    rows = report['rows']
    require(len(rows) == 2 and {r['family_id'] for r in rows} == FAMILIES, 'Expected two reserved families')
    manifest = read_json(ROOT / 'fixtures/drawing-manifest.json')
    results = []
    for row in rows:
        family = row['family_id']
        require(row['status'] == 'captured', 'Native acquisition failed: ' + family)
        require(row['split'] == 'holdout', 'Lost holdout designation')
        assets = [a for a in manifest['assets'] if a['family_id'] == family]
        require(len(row['inputs']) == len(assets)
                and {a['file'] for a in assets} == {a['upstream_path'] for a in row['inputs']}, 'Incomplete input provenance')
        base = root / family
        for asset in assets:
            original = base / 'original' / Path(asset['file']).name
            expected = dict(file_name=original.name, bytes=asset['bytes'], sha256=asset['sha256'])
            provenance = next(a for a in row['inputs'] if a['upstream_path'] == asset['file'])
            require(provenance['url'] == asset['url'] and provenance['identity'] == expected, 'Input provenance mismatch')
            require(not original.is_symlink() and identity(original) == expected, 'Changed original: ' + str(original))
        boundary = dict(row, sheet=row['sheets'], sheet_after=row['sheets_after'])
        saved_boundary(boundary, sheet_count=len(row['sheets']))
        local_file(base / 'major31', row['source'], '.idw')
        local_file(base / 'major31', row['pdf'], '.pdf')
        require(all(r['missing'] is False for r in row['references']), 'Missing native reference')
        for sheet in row['sheets']:
            for view in sheet['views']:
                require(view['up_to_date']['status'] == 'captured' and view['up_to_date'].get('value') is True,
                        'Native view is not current')
                count = view['curve_count']
                require(count['status'] == 'captured' and type(count.get('value')) is int and count['value'] >= 0,
                        'Native view curve count unavailable')
        failures = [p for p, value in walk(row) if isinstance(value, dict) and value.get('status') == 'failed']
        require(not failures, 'Native property failures: ' + ', '.join(failures))
        results.append(dict(family_id=family, source=row['source'], pdf=row['pdf'], sheet_count=len(row['sheets']),
                            reference_count=len(row['references']), status='acquisition_identity_checked', decoder_evaluated=False))
    return dict(schema_version=1, status='acquisition_checks_passed', qualified_oracle=False,
                independent_qualified_holdout_families=[], results=results,
                limitations=['Native major31 framing and full oracle/visual acceptance remain unverified.',
                             'Do not use these reserved families for decoder or tolerance development.'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Choose a new output path')
    result = validate(args.input)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8') as stream:
        json.dump(result, stream, indent=2, ensure_ascii=False, allow_nan=False)
        stream.write('\n')
    print(result['status'])


if __name__ == '__main__':
    main()
