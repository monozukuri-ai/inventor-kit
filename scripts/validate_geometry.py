"""Pinned corpus regression, with bounded geometry and explicit failures."""
import argparse
from collections import Counter
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import time

import cq_acis
from cq_acis import CadQueryConverter, CadQueryConversionError, RawEntity
import inventor_kit
from corpus_manifest import load_manifest
from oracle_contract import shape_metrics

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--split', choices=['all', 'regression', 'holdout'], default='all')
    parser.add_argument('--output', type=Path, default=ROOT/'internal/reports/latest/geometry_validation.json')
    parser.add_argument('--baseline', type=Path, default=ROOT/'tests/data/geometry-baseline.json',
                        help='Pinned regression metrics; never requires a previous local run')
    args = parser.parse_args()
    if args.baseline.resolve() == args.output.resolve():
        parser.error('--output must not overwrite the baseline')
    results = []
    for item in load_manifest(split=args.split):
        data = (ROOT/'fixtures/public'/item['file']).read_bytes()
        if hashlib.sha256(data).hexdigest() != item['sha256'] or len(data) != item['bytes']:
            raise ValueError(f"Fixture mismatch: {item['file']}")
        doc = inventor_kit.read(data, source_id=item['file'])
        entry = {key: item[key] for key in ('file', 'sha256', 'split', 'family_id')}
        entry.update(converted=False, current_state='unverified', vendor_comparison='not_collected')
        if doc.model is None:
            entry.update(status='geometry_unavailable', error_code='geometry.unavailable')
        else:
            entry['entities'] = dict(sorted(Counter(type(e).__name__ for e in doc.model.entities).items()))
            entry['bodies'] = len(doc.model.bodies())
            entry['raw_geometry_subtypes'] = dict(sorted(Counter(
                e.type_name + ':' + next((v for v in e.values if isinstance(v, str)), 'unnamed')
                for e in doc.model.entities if isinstance(e, RawEntity)
                and e.type_name in ('spline-surface', 'intcurve-curve', 'pcurve')
            ).items()))
            converter = CadQueryConverter(doc.model)
            started = time.perf_counter()
            try:
                shapes = converter.convert()
                if not shapes:
                    raise CadQueryConversionError('No decoded bodies in stored table', code='geometry.no_bodies')
                if not all(s.isValid() and s.Solids() and s.Volume() > 0 for s in shapes):
                    raise AssertionError(f"Empty, invalid or non-positive solid: {item['file']}")
                entry.update(converted=True, status='converted_valid', metrics=shape_metrics(shapes))
            except CadQueryConversionError as error:
                entry.update(status='unsupported', error_code=error.code, error=str(error))
            entry['conversion_seconds'] = time.perf_counter() - started
            entry['pcurve_validation'] = {'count': converter.pcurve_count,
                'max_deviation_mm': converter.pcurve_max_deviation,
                'source_tolerance_mm': converter.tolerance,
                'method': 'OCCT GeomLib_CheckCurveOnSurface, same-parameter extrema'}
            entry['degenerate_edges'] = converter.degenerate_edges
            entry['analytic_trim_faces'] = converter.analytic_trim_faces
            entry['tolerant_endpoints'] = converter.tolerant_endpoints
            entry['resolved_subtypes'] = converter.resolved_subtypes
            entry['subtype_failures'] = converter.subtype_failures
            assert converter.pcurve_max_deviation <= converter.tolerance
        results.append(entry)
        print(item['file'], entry['status'], entry.get('error_code', ''))
    if args.baseline:
        prior = {e['file']: e for e in json.loads(args.baseline.read_text())['results']}
        for entry in results:
            old = prior.get(entry['file'])
            if old is None or not old['converted']:
                continue
            if old['sha256'] != entry['sha256'] or entry['split'] != 'regression':
                raise AssertionError(f"Baseline source/split mismatch: {entry['file']}")
            if not entry['converted']:
                raise AssertionError(f"Conversion regression: {entry['file']}")
            before, after = old['metrics'], entry['metrics']
            if before['solid_count'] != after['solid_count']:
                raise AssertionError(f"Solid count regression: {entry['file']}")
            for key in ['volume_mm3', 'area_mm2']:
                if not math.isclose(before[key], after[key], rel_tol=1e-9, abs_tol=1e-6):
                    raise AssertionError(f"{key} regression: {entry['file']}")
            if any(abs(a-b) > 1e-6 for a,b in zip(before['bbox_mm'],after['bbox_mm'])):
                raise AssertionError(f"Bounding box regression: {entry['file']}")
            entry['metric_regression'] = 'passed'
    report = {'scope': 'stored B-rep tables; current Inventor model state remains unverified',
        'model_api_version': cq_acis.model.MODEL_API_VERSION,
        'core_version': cq_acis._native.CORE_VERSION,
        'packages': {name: importlib.metadata.version(name) for name in ['inventor-kit','cq-acis','cadquery','cadquery-ocp']},
        'module_files': {'cq_acis':cq_acis.__file__, 'inventor_kit':inventor_kit.__file__},
        'baseline_sha256': hashlib.sha256(args.baseline.read_bytes()).hexdigest(),
        'splits': {split: {'files': sum(e['split']==split for e in results),
                          'converted': sum(e['split']==split and e['converted'] for e in results)}
                   for split in ['regression','holdout']},
        'error_counts': dict(sorted(Counter(e['error_code'] for e in results if 'error_code' in e).items())),
        'results':results}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report['splits']))


if __name__ == '__main__':
    main()
