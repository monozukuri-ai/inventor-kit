"""Validate independent captures and compare metrics without asserting active state."""
from pathlib import Path
import json
import math

SCHEMA = Path(__file__).resolve().parents[1] / 'schemas/vendor-oracle-v1.schema.json'


def validate_capture(data, *, source_sha256=None):
    from jsonschema import Draft202012Validator, FormatChecker
    schema = json.loads(SCHEMA.read_text())
    Draft202012Validator(schema, format_checker=FormatChecker()).validate(data)

    def finite(value):
        if isinstance(value, float) and not math.isfinite(value):
            raise ValueError('Nonfinite oracle value')
        if isinstance(value, dict):
            for child in value.values():
                finite(child)
        if isinstance(value, list):
            for child in value:
                finite(child)
    finite(data)
    if source_sha256 is not None and data['source']['sha256'] != source_sha256:
        raise ValueError('Oracle belongs to a different source SHA-256')
    g = data['geometry']
    if g['status'] == 'available':
        if any(g['bbox_mm'][i] > g['bbox_mm'][i+3] for i in range(3)):
            raise ValueError('Invalid oracle bounding box')
        if g['solid_count'] > g['body_count']:
            raise ValueError('Oracle solid count exceeds its surface body count')
    return data


def load_capture(path, *, source_sha256=None):
    def invalid(value):
        raise ValueError(f'Nonfinite JSON constant: {value}')
    return validate_capture(json.loads(Path(path).read_text(), parse_constant=invalid), source_sha256=source_sha256)


def shape_metrics(shapes):
    """Aggregate OCCT output, retaining ACIS body counts separately in the report."""
    boxes = [s.BoundingBox() for s in shapes]
    return {
        'solid_count': sum(len(s.Solids()) for s in shapes),
        'faces': sum(len(s.Faces()) for s in shapes),
        'volume_mm3': sum(s.Volume() for s in shapes),
        'area_mm2': sum(s.Area() for s in shapes),
        'bbox_mm': [min(getattr(b, k) for b in boxes) for k in ('xmin', 'ymin', 'zmin')]
                   + [max(getattr(b, k) for b in boxes) for k in ('xmax', 'ymax', 'zmax')],
    }


def compare_capture(capture, actual, *, rtol=1e-8, length_atol=1e-6, area_atol=1e-6, volume_atol=1e-6):
    validate_capture(capture)
    for value in (rtol, length_atol, area_atol, volume_atol):
        if not math.isfinite(value) or value < 0:
            raise ValueError('Comparison tolerances must be finite and nonnegative')
    result = {
        'provider': capture['provider'],
        'status': 'not_compared',
        'state_verification': 'unverified',
        'scope': capture['capture']['scope'],
        'checks': {},
        'tolerances': dict(rtol=rtol, length_atol_mm=length_atol, area_atol_mm2=area_atol, volume_atol_mm3=volume_atol),
        'limitations': ['Aggregate metrics do not establish body identity, full geometric equality, or active-state ownership.',
                        'Native SurfaceBody and ACIS body are different identity domains; body counts and face counts are recorded, not equated.'],
    }
    reference = capture['geometry']
    if actual is None or reference['status'] != 'available':
        return result
    checks = result['checks']
    checks['solid_count'] = actual['solid_count'] == reference['solid_count']
    for key, atol in [('volume_mm3', volume_atol), ('area_mm2', area_atol)]:
        checks[key] = math.isclose(actual[key], reference[key], rel_tol=rtol, abs_tol=atol)
    if reference['bbox_kind'] == 'precise':
        checks['bbox_mm'] = all(math.isclose(a, b, rel_tol=rtol, abs_tol=length_atol) for a, b in zip(actual['bbox_mm'], reference['bbox_mm'], strict=True))
    else:
        result['limitations'].append('The vendor box is not marked precise; it is not an equality oracle.')
    result['status'] = 'matched_metrics' if all(checks.values()) else 'mismatch'
    # Even a declared active-state capture cannot prove that our selected carrier
    # belongs to that state. M3 must establish that relationship independently.
    return result
