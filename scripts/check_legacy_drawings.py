"""Check private IDW drawings of segment majors 11-14 kept in a local directory.

These Meta6/7 profiles were framed from production drawings without native
captures, so no regression input is distributed. Point ``--input`` (or the
``INVENTOR_KIT_LEGACY_DRAWINGS`` environment variable) at a local directory.
Each line of the output names a file only by a SHA-256 prefix and reports
structural facts: status, segment majors, sheet/item/view counts and the
diagnostic codes. No decoded text is written.

The check fails when a drawing with a major 11-14 segment raises, reports a
Meta6/7 layer-marker mismatch, or ends without a stored display while no
resource limit was exceeded. It does not qualify units, state or rendering.
"""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import sys

LEGACY_MAJORS = range(11, 15)


def check(path):
    from inventor_kit import read_drawing_file

    row = dict(file=hashlib.sha256(path.read_bytes()).hexdigest()[:16])
    try:
        drawing = read_drawing_file(path)
    except Exception as exc:  # The report keeps going; the type is enough.
        row.update(status='error', error=type(exc).__name__)
        return row, True
    majors = sorted({s.major for s in drawing.metadata.segments})
    messages = [d['message'] for d in drawing.diagnostics]
    row.update(
        status=drawing.status,
        majors=majors,
        sheets=len(drawing.sheets),
        items=sum(len(s.items) for s in drawing.sheets),
        views=sum(len(s.views) for s in drawing.sheets),
        diagnostics=dict(Counter(d['code'] for d in drawing.diagnostics)),
    )
    legacy = any(m in LEGACY_MAJORS for m in majors)
    marker = any('Meta6/7 layer marker' in m for m in messages)
    limited = any('limit exceeded' in m for m in messages)
    failed = legacy and (marker or (drawing.status != 'experimental_partial' and not limited))
    return row, failed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path,
                        default=os.environ.get('INVENTOR_KIT_LEGACY_DRAWINGS'))
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.input is None:
        parser.error('--input or INVENTOR_KIT_LEGACY_DRAWINGS is required')
    paths = sorted(p for p in Path(args.input).rglob('*') if p.suffix.lower() == '.idw')
    if not paths:
        parser.error(f'no .idw files under {args.input}')
    rows, failures = [], 0
    for path in paths:
        row, failed = check(path)
        row['passed'] = not failed
        failures += failed
        rows.append(row)
        print(json.dumps(row, sort_keys=True), flush=True)
    if args.output:
        args.output.write_text(''.join(json.dumps(r, sort_keys=True) + '\n' for r in rows))
    print(f'{len(rows) - failures} of {len(rows)} drawings passed', file=sys.stderr)
    raise SystemExit(1 if failures else 0)


if __name__ == '__main__':
    main()
