"""Offline drawing corpus/observation checks. Missing native evidence never passes."""
import argparse
import json
from pathlib import Path

from corpus_manifest import ROOT
from drawing_corpus import drawing_rows
from drawing_oracle_contract import identity, load_capture, readiness


def inventory(fixtures, captures=None):
    fixtures = Path(fixtures).resolve()
    records = []
    for entry, row in drawing_rows():
        target = (fixtures / row['file']).resolve()
        if not target.is_relative_to(fixtures):
            raise ValueError('Drawing path escapes fixture root')
        actual = identity(target)
        if (actual['bytes'], actual['sha256']) != (row['bytes'], row['sha256']):
            raise ValueError(f"Drawing identity mismatch: {row['file']}")
        path = Path(captures) / (row['sha256'] + '.json') if captures else None
        result = dict(id=entry['id'], file=row['file'], sha256=row['sha256'], role=entry['role'],
                      family_id=row['family_id'], split=row['split'], source_integrity='passed',
                      oracle='not_collected', comparison=dict(status='incomplete', reasons=['not_collected']))
        if path is not None and path.is_file():
            data = load_capture(path, source=target, artifact_root=path.parent)
            result.update(oracle=data['provider']['name'], comparison=readiness(data))
        records.append(result)
    return dict(source_integrity='passed', acquisition='ready' if all(r['comparison']['status'] == 'ready' for r in records) else 'incomplete',
                drawings=len(records), native_captures=sum(r['oracle'].startswith('autodesk.') for r in records), results=records)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inventory', action='store_true', help='Report acquisition gaps; exit success for corpus integrity only')
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'fixtures/public')
    parser.add_argument('--captures', type=Path, help='Directory of <source-sha256>.json captures and relative visual artifacts')
    parser.add_argument('--capture', type=Path, help='Validate one acquired observation JSON')
    parser.add_argument('--source', type=Path, help='Required for single-capture identity validation')
    parser.add_argument('--artifact-root', type=Path)
    parser.add_argument('--require-ready', action='store_true', help='Require complete native acquisition evidence, including saved-state/visual binding')
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.capture:
        if args.source is None or args.inventory or args.captures:
            parser.error('--capture requires --source and cannot be combined with --inventory/--captures')
        data = load_capture(args.capture, source=args.source, artifact_root=args.artifact_root or args.capture.parent)
        report = dict(contract='valid', source_integrity='passed', provider=data['provider']['name'], acquisition=readiness(data))
        passed = not args.require_ready or report['acquisition']['status'] == 'ready'
    else:
        if args.source or args.artifact_root:
            parser.error('--source/--artifact-root require --capture')
        report = inventory(args.fixtures, args.captures)
        passed = (args.inventory and not args.require_ready) or report['acquisition'] == 'ready'
    payload = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False)+'\n'
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload, encoding='utf-8')
    print(payload, end='')
    raise SystemExit(0 if passed else 1)


if __name__ == '__main__':
    main()
