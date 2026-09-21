"""Saved-drawing acceptance report. Inventory/regression is not native qualification.

By default, missing captures, unverified units, unimplemented feature comparisons,
or fewer than two independently qualified holdout families are failures. --inventory
only checks pinned input identities and reports these gaps; it never qualifies them.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

from corpus_manifest import ROOT
from drawing_corpus import drawing_rows
from drawing_oracle_contract import identity, load_capture, readiness, read_json

POLICY = ROOT / 'tests/data/drawing-acceptance.json'


def compare_document(doc, capture, policy):
    """Compare independent native observations without fitting source units."""
    checks, gaps = [], []
    def check(name, actual, expected, passed):
        checks.append(dict(check=name, actual=actual, expected=expected, status='passed' if passed else 'failed'))
    sheets = capture['sheets']
    if sheets['status'] != 'captured':
        return dict(status='incomplete', checks=[], gaps=['native_sheet_list_incomplete'])
    observed = [item['result']['value'] for item in sheets['items']]
    check('sheet_count', len(doc.sheets), len(observed), len(doc.sheets) == len(observed))
    scale = doc.millimeters_per_unit
    if scale is None or doc.length_unit is None or not math.isfinite(scale) or scale <= 0:
        gaps.append('physical_units_unverified')
        scale = None
    for index, (sheet, native) in enumerate(zip(doc.sheets, observed)):
        # Inventor may decorate a stored name. Do not silently strip suffixes on
        # unrelated drawings; retain both values until a naming rule is qualified.
        name = native['name'].get('value')
        if name is None:
            gaps.append(f'sheet_{index}:native_name_unavailable')
        elif name != sheet.name:
            gaps.append(f'sheet_{index}:stored_and_native_name_relationship_unverified')
        else:
            check(f'sheet_{index}:name', sheet.name, name, True)
        width, height = (native[k].get('value') for k in ('width', 'height'))
        if scale is not None and sheet.size_in_source_units is not None and width is not None and height is not None:
            actual = [v * scale for v in sheet.size_in_source_units]
            expected = [width * 10, height * 10]  # Oracle contract explicitly uses cm.
            check(f'sheet_{index}:size_mm', actual, expected,
                  all(abs(a - b) <= policy['tolerances']['sheet_size_mm'] for a, b in zip(actual, expected)))
        else:
            gaps.append(f'sheet_{index}:paper_size_comparison_unavailable')
        if sheet.content_coverage != 'complete':
            gaps.append(f'sheet_{index}:content_coverage_{sheet.content_coverage}')
        if sheet.status == 'unavailable':
            gaps.append(f'sheet_{index}:display_unavailable')
    # A successful inventory/size comparison cannot stand in for these checks.
    gaps.extend('comparison_not_qualified:' + feature for feature in policy['required_features']
                if feature not in ('sheet_identity_order', 'physical_units'))
    return dict(status='failed' if any(c['status'] == 'failed' for c in checks) else
                'incomplete' if gaps else 'passed', checks=checks, gaps=gaps)


def evaluate(fixtures, captures=None, policy_path=POLICY):
    from inventor_kit import read_drawing_file
    policy = read_json(policy_path)
    if policy['schema_version'] != 1 or policy['minimum_independent_holdout_families'] < 2:
        raise ValueError('Invalid drawing acceptance policy')
    root = Path(fixtures).resolve()
    results = []
    for entry, row in drawing_rows():
        source = (root / row['file']).resolve()
        if not source.is_relative_to(root):
            raise ValueError('Drawing escapes fixture root')
        actual = identity(source)
        if (actual['bytes'], actual['sha256']) != (row['bytes'], row['sha256']):
            raise ValueError('Drawing fixture identity mismatch: ' + row['file'])
        result = dict(file=row['file'], sha256=row['sha256'], family_id=row['family_id'], split=row['split'],
                      input_integrity='passed', acquisition=dict(status='incomplete', reasons=['capture_not_collected']),
                      comparison=dict(status='not_run', gaps=['native_capture_required']))
        path = Path(captures) / (row['sha256'] + '.json') if captures else None
        if path is not None and path.is_file():
            capture = load_capture(path, source=source, artifact_root=path.parent)
            result['acquisition'] = readiness(capture)
            # Holdouts are not even decoded until independent acquisition is ready.
            if row['split'] != 'holdout' or result['acquisition']['status'] == 'ready':
                result['comparison'] = compare_document(read_drawing_file(source), capture, policy)
        results.append(result)
    families = sorted({r['family_id'] for r in results if r['split'] == 'holdout'
                       and all(s['acquisition']['status'] == 'ready' and s['comparison']['status'] == 'passed'
                               for s in results if s['family_id'] == r['family_id'])})
    qualified = (len(families) >= policy['minimum_independent_holdout_families']
                 and all(r['acquisition']['status'] == 'ready' and r['comparison']['status'] == 'passed' for r in results)
                 and policy['tolerances']['visual_comparison'] == 'qualified')
    return dict(schema_version=1, status='passed' if qualified else 'incomplete', qualified=qualified,
                input_integrity='passed', policy_sha256=hashlib.sha256(Path(policy_path).read_bytes()).hexdigest(),
                independent_holdout_families= families, required_holdout_families=policy['minimum_independent_holdout_families'],
                results=results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=ROOT/'fixtures/public')
    parser.add_argument('--captures', type=Path)
    parser.add_argument('--inventory', action='store_true', help='Success for input integrity only; qualification stays incomplete')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    report = evaluate(args.fixtures, args.captures)
    payload = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + '\n'
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload, encoding='utf-8')
    print(payload, end='')
    raise SystemExit(0 if args.inventory or report['qualified'] else 1)


if __name__ == '__main__':
    main()
