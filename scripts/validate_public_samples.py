#!/usr/bin/env python3
"""Offline corpus validation. Missing files fail, rather than shrinking coverage."""
from pathlib import Path
import argparse
import hashlib
import importlib.metadata
import json
import subprocess
import tempfile

import inventor_kit
from cq_acis import RawEntity, CadQueryConversionError
from corpus_manifest import load_manifest
from oracle_contract import load_capture, compare_capture, shape_metrics

ROOT = Path(__file__).resolve().parents[1]


def package_version(name):
    try:
        return importlib.metadata.version(name)
    except importlib.metadata.PackageNotFoundError:
        return None  # In-place native builds may have no installed metadata.


def report_document(value):
    """Keep binary provenance compact; the public API retains original bytes."""
    if isinstance(value, dict):
        return {key: ({'bytes': len(bytes.fromhex(child)), 'sha256': hashlib.sha256(bytes.fromhex(child)).hexdigest()}
                      if key in ('raw_data', 'data') and isinstance(child, str) else report_document(child))
                for key, child in value.items()}
    if isinstance(value, list):
        return [report_document(child) for child in value]
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'fixtures/public')
    parser.add_argument('--output', type=Path, default=ROOT / 'internal/reports/latest/public_validation.json')
    parser.add_argument('--split', choices=['all', 'regression', 'holdout'], default='all')
    parser.add_argument('--oracle-dir', type=Path, help='Captures named <source SHA-256>.json')
    parser.add_argument('--oracle-python', help='Python containing ezdxf; compares active-record framing, not geometry')
    args = parser.parse_args()
    if args.oracle_dir is not None and not args.oracle_dir.is_dir():
        parser.error('--oracle-dir must be an existing directory')
    results, failures = [], []
    for item in load_manifest(split=args.split):
        data = (args.fixtures / item['file']).read_bytes()
        if len(data) != item['bytes'] or hashlib.sha256(data).hexdigest() != item['sha256']:
            raise ValueError(f"Fixture mismatch: {item['file']}")
        doc = inventor_kit.read(data, source_id=item['file'])
        summary = {**doc.summary, 'document': report_document(doc.summary['document'])}
        result = {'file': item['file'], 'sha256': item['sha256'], 'split': item['split'], 'family_id': item['family_id'], 'summary': summary, 'converted': False,
                  'stages': {'container': 'passed',
                             'carrier_selection': 'selected' if doc.summary['carrier'] else 'unavailable',
                             'typed_decode': 'decoded_subset' if doc.model is not None else 'unavailable',
                             'geometry': 'not_attempted', 'current_state': 'unverified'},
                  'vendor_comparison': {'status': 'not_collected', 'state_verification': 'unverified'}}
        aggregate = None
        if doc.model is not None:
            result['records'] = len(doc.model)
            result['bodies'] = len(doc.model.bodies())
            result['diagnostics'] = [dict(code=d.code, message=d.message, entity_index=d.entity_index) for d in doc.model.diagnostics]
            raw = [e if isinstance(e, RawEntity) else e.raw for e in doc.model.entities]
            carrier = doc.summary['carrier']
            for r in raw:
                span = r.source
                assert r.raw_data == doc.kernel_bytes[span.start_offset-carrier['kernel_offset']:span.end_offset-carrier['kernel_offset']]
                assert span.source_id == carrier['source_id']
            result['source_spans_verified'] = len(raw)
            active = [r for r in raw if r.type_name != '__opaque_sab_history__']
            if args.oracle_python:
                with tempfile.TemporaryDirectory() as tmp:
                    path = Path(tmp) / 'kernel.sab'
                    path.write_bytes(doc.kernel_bytes)
                    completed = subprocess.run([args.oracle_python, str(ROOT / 'scripts/ezdxf_oracle.py'), str(path), str(len(active))], capture_output=True, text=True, check=True)
                oracle = json.loads(completed.stdout)
                expected = [[r.type_name, r.attributes.index, r.entity_id, r.source.start_offset-carrier['kernel_offset'], r.source.end_offset-carrier['kernel_offset']] for r in active]
                assert oracle['records'] == expected, item['file']
                assert oracle['save_version'] == carrier['save_version']
                result['ezdxf_active_records_verified'] = len(active)
            try:
                shapes = doc.to_cadquery().vals()
                result['geometry'] = [dict(valid=s.isValid(), volume_mm3=s.Volume(), area_mm2=s.Area(), faces=len(s.Faces()), solids=len(s.Solids()), bbox_mm=[getattr(s.BoundingBox(), k) for k in ('xmin', 'ymin', 'zmin', 'xmax', 'ymax', 'zmax')]) for s in shapes]
                if not shapes or not all(s.isValid() for s in shapes):
                    raise AssertionError(f'Converter returned empty/invalid geometry: {item["file"]}')
                aggregate = shape_metrics(shapes)
                result['aggregate_geometry'] = aggregate
                result['converted'] = True
                result['stages']['geometry'] = 'converted_valid'
            except (CadQueryConversionError, ValueError) as error:
                result['conversion_error'] = f'{type(error).__name__}: {error}'
                result['conversion_error_code'] = getattr(error, 'code', 'geometry.unavailable')
                result['stages']['geometry'] = 'unsupported'
        if args.oracle_dir:
            oracle = args.oracle_dir / (item['sha256'] + '.json')
            if oracle.exists():
                capture = load_capture(oracle, source_sha256=item['sha256'])
                result['vendor_comparison'] = compare_capture(capture, aggregate)
                if result['vendor_comparison']['status'] == 'mismatch':
                    failures.append(f'{item["file"]}: independent metric mismatch')
        baseline = item.get('baseline')
        if baseline:
            if baseline['status'] == 'decoded_subset' and doc.model is None:
                failures.append(f'{item["file"]}: shared model regression')
            if baseline['records'] is not None and result.get('records') != baseline['records']:
                failures.append(f'{item["file"]}: stored record count changed')
            if baseline['converted']:
                if not result['converted']:
                    failures.append(f'{item["file"]}: shape conversion regression')
                else:
                    import math
                    if len(result['geometry']) != len(baseline['geometry']):
                        failures.append(f'{item["file"]}: output body count changed')
                    for now, before in zip(result['geometry'], baseline['geometry']):
                        for key in ('volume_mm3', 'area_mm2'):
                            if not math.isclose(now[key], before[key], rel_tol=1e-8, abs_tol=1e-6):
                                failures.append(f'{item["file"]}: {key} regression')
                        if now['solids'] != before['solids'] or any(not math.isclose(a,b,rel_tol=1e-8,abs_tol=1e-6) for a,b in zip(now['bbox_mm'],before['bbox_mm'])):
                            failures.append(f'{item["file"]}: topology/bounds regression')
        results.append(result)
        print(item['file'], doc.summary['status'], 'converted' if result['converted'] else result.get('conversion_error', 'inventory only'), flush=True)
    report = {'scope': 'Stored B-rep table only; history is opaque, no Inventor SDK/current-state oracle.', 'versions': {name: package_version(name) for name in ('inventor-kit', 'cq-acis', 'cadquery')}, 'total': len(results), 'decoded_subset': sum(r['summary']['status']=='decoded_subset' for r in results), 'converted': sum(r['converted'] for r in results), 'by_split': {split: {'total': sum(r['split']==split for r in results), 'decoded_subset': sum(r['split']==split and r['summary']['status']=='decoded_subset' for r in results), 'converted': sum(r['split']==split and r['converted'] for r in results)} for split in ('regression','holdout')},
              'failures': failures, 'results': results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False)+'\n')
    if failures:
        raise SystemExit('Corpus regressions: ' + '; '.join(failures))


if __name__ == '__main__':
    main()
