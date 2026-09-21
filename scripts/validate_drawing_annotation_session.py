"""Pinned Inventor annotation controls: bounded API/bitmap checks, not qualification.

Binary inputs remain local. A successful exit does not qualify physical units,
font substitution, independent holdouts, or the formal drawing-oracle collector.
"""
import argparse
from collections import Counter
import hashlib
import io
import json
import math
from pathlib import Path
import tempfile

from drawing_oracle_contract import identity, read_json, walk
from validate_drawing_control_session import near

MANIFEST = Path(__file__).resolve().parents[1] / 'tests/data/drawing-annotation-controls.json'


def check(name, ok, **evidence):
    return dict(check=name, status='passed' if ok else 'failed', **evidence)


def captured(value):
    if value['status'] != 'captured':
        raise ValueError('Expected captured API field')
    return value['value']


def line_segments(items):
    return [g['points'] for i in items if (g := i.geometry)['kind'] == 'polyline' and len(g['points']) == 2]


def line_coverage(lines, a, b):
    """Require the union of collinear saved segments to cover the API segment."""
    direction = [b[i]-a[i] for i in range(2)]
    length = math.hypot(*direction)
    if length == 0:
        return False
    direction = [v/length for v in direction]
    intervals = []
    for pair in lines:
        if all(abs((p[0]-a[0])*direction[1]-(p[1]-a[1])*direction[0]) < 1e-6 for p in pair):
            intervals.append(sorted(sum((p[i]-a[i])*direction[i] for i in range(2)) for p in pair))
    end = 0.
    for lo, hi in sorted(intervals):
        if hi < end:
            continue
        if lo > end+1e-6:
            return False
        end = hi
        if end >= length-1e-6:
            return True
    return False


def raster_checks(sheet, images, views):
    from PIL import Image
    rasters = [i.geometry for i in sheet.items if i.geometry['kind'] == 'image' and i.geometry['format'] == 3]
    checks = [check('saved_view_bitmap_count', len(rasters) == len(views), actual=len(rasters), expected=len(views))]
    checks.append(check('saved_view_names_and_order', [v.name for v in sheet.views] == [v['name'] for v in views]))
    checks.append(check('saved_view_semantic_unknowns_retained', all(v.rotation is None and v.view_type is None and v.parent_view_id is None for v in sheet.views)))
    if len(rasters) != len(views):
        return checks
    for index, (g, view) in enumerate(zip(rasters, views)):
        image = images.get(g['reference'])
        checks.append(check('view_bitmap_available', image is not None and image.data is not None, view=index))
        if image is None or image.data is None:
            continue
        with Image.open(io.BytesIO(image.data)) as decoded:
            decoded.load()
            alpha = decoded.convert('RGBA').getchannel('A')
        # Transform the independently observed API points through the IDW image
        # basis. Never fit to the API/PDF or to the bitmap's occupied bounding box.
        origin, u, v = (g[k] for k in ('origin', 'u', 'v'))
        determinant = u[0]*v[1]-u[1]*v[0]
        if abs(determinant) < 1e-12:
            checks.append(check('view_bitmap_basis', False, view=index))
            continue
        hits = total = hidden = 0
        for curve in view['curves']:
            for segment in curve['segments']:
                if not captured(segment['visible']):
                    continue
                if captured(segment['hidden']):
                    hidden += 1  # Dash gaps are compared visually, not as solid lines.
                    continue
                shape = captured(segment['geometry'])
                if shape['kind'] != 'line':
                    checks.append(check('view_geometry_kind', False, view=index, kind=shape['kind']))
                    continue
                for j in range(17):
                    point = [shape['start'][k]+(shape['end'][k]-shape['start'][k])*j/16-origin[k] for k in range(2)]
                    x = (point[0]*v[1]-point[1]*v[0])/determinant*alpha.width
                    y = (u[0]*point[1]-u[1]*point[0])/determinant*alpha.height
                    # Quantized cache at its native resolution, not vector accuracy.
                    px, py = math.floor(x), math.floor(y)
                    hits += any(alpha.getpixel((a, b)) > 0 for a in range(max(0,px-2),min(alpha.width,px+3))
                                for b in range(max(0,py-2),min(alpha.height,py+3)))
                    total += 1
        checks.append(check('api_visible_lines_in_saved_bitmap', total > 0 and hits == total, view=index,
                            hits=hits, samples=total, tolerance_cache_pixels=2, hidden_segments_not_sampled=hidden))
    return checks


def table_checks(items, tables):
    texts = [i.geometry for i in items if i.geometry['kind'] == 'text']
    lines = line_segments(items)
    result = []
    for table in tables:
        lo, hi = table['range_min'], table['range_max']
        corners = [lo, [hi[0],lo[1]], hi, [lo[0],hi[1]]]
        result.append(check('parts_list_border_api_range', all(line_coverage(lines,a,b) for a,b in zip(corners,corners[1:]+corners[:1]))))
        result.append(check('parts_list_title', sum(g['text'] == table['title'] for g in texts) == 1))
        left = lo[0]
        for index, col in enumerate(table['columns']):
            right = left + col['width']
            actual = [g['text'].strip() for g in sorted(texts,key=lambda g:-g['position'][1])
                      if left < g['position'][0] < right and g['text'] != table['title']]
            expected = [col['title']] + [captured(row['cells'][index]) for row in table['rows']]
            result.append(check('parts_list_column_text_and_order', actual == expected, column=index, actual=actual, expected=expected))
            left = right
        result.append(check('parts_list_column_width_sum', near(left,hi[0])))
    return result


def display_signature(doc):
    from inventor_kit.drawing import _plain
    result = []
    for sheet in doc.sheets:
        items = []
        for item in sheet.items:
            geometry, style = _plain(item.geometry), _plain(item.style)
            # Provenance source_id follows the new path; display values/IDs must not.
            if geometry.get('font') is not None:
                geometry['font'].pop('source')
            style.pop('sources')
            items.append((item.id, geometry, style))
        views = [(v.id, v.name, v.placement_transform, v.cache_bounds, v.image_reference, v.item_ids)
                 for v in sheet.views]
        result.append((sheet.id, sheet.name, sheet.size_in_source_units, items, views))
    return result


def validate(root):
    from inventor_kit import read_drawing_file
    root = Path(root).resolve()
    manifest = read_json(MANIFEST)
    for record in manifest['files']:
        name = record['file_name']
        if Path(name).name != name or '\\' in name:
            raise ValueError('Unsafe manifest name')
        path = root/name
        if path.is_symlink() or identity(path) != record:
            raise ValueError('Pinned input identity mismatch: '+name)
    capture = read_json(root/'annotations.native.json')
    if (capture['format'] != 'inventor-kit-native-annotations-v1' or capture['qualified_oracle'] is not False
            or len(capture['rows']) != 11 or len({r['case'] for r in capture['rows']}) != 11):
        raise ValueError('Unexpected native session')
    results = []
    for row in capture['rows']:
        if row['status'] != 'captured':
            raise ValueError('Incomplete capture row')
        name = row['source']['file']
        doc = read_drawing_file(root/name)
        checks = [check('sheet_count',len(doc.sheets) == 1),
                  check('remains_unqualified',not doc.qualified and not doc.complete and doc.millimeters_per_unit is None)]
        native = row['sheet']
        if len(doc.sheets) == 1:
            sheet = doc.sheets[0]
            checks += [check('stored_name_api_base_name', native['name'] == sheet.name+':1', stored=sheet.name, api=native['name']),
                       check('raw_size_api_cm_correlation',near(sheet.size_in_source_units,[native['width_cm'],native['height_cm']])),
                       check('partial_display_available',sheet.status == 'experimental_partial')]
            texts = [i.geometry for i in sheet.items if i.geometry['kind'] == 'text']
            lines = line_segments(sheet.items)
            for dimension in native['dimensions']:
                actual = ''.join(g['text'] for g in texts)
                checks.append(check('raw_dimension_text',actual == captured(dimension['text']),actual=actual,expected=captured(dimension['text'])))
                for key in ('extension_one','extension_two'):
                    if dimension[key]['status'] == 'captured':
                        g = captured(dimension[key]); checks.append(check('dimension_'+key,line_coverage(lines,g['start'],g['end'])))
                if row['case'] == 'dimension-linear':
                    g = captured(dimension['dimension_line']); checks.append(check('dimension_line_union',line_coverage(lines,g['start'],g['end'])))
                else:
                    # The radial API DimensionLine is the model diameter, not
                    # the elbow leader that is actually printed. Check its tip.
                    tip = captured(dimension['dimension_line'])['start']
                    checks.append(check('diameter_arrow_tip',any(near(p[:2],tip) for line in lines for p in line)))
                    checks.append(check('diameter_legacy_font_retained',any(g['text']=='n' and g['font']['family']=='AIGDT' for g in texts)))
            for leader in native['leaders']:
                checks.append(check('leader_text',''.join(g['text'] for g in texts) == captured(leader['text'])))
                lo, hi = captured(leader['range_min']), captured(leader['range_max'])
                checks.append(check('leader_baseline_inside_api_range',all(lo[0]-1e-6 <= g['position'][0] <= hi[0]+1e-6
                    and lo[1]-1e-6 <= g['position'][1] <= hi[1]+1e-6 for g in texts)))
            for block in native['blocks']:
                if block['texts']['status'] == 'captured':
                    expected = Counter(captured(v) for v in captured(block['texts']) if captured(v))
                    checks.append(check('block_result_text', Counter(g['text'] for g in texts) == expected))
            checks += table_checks(sheet.items,native['parts_lists'])
            checks += raster_checks(sheet,{i.reference:i for i in doc.images},native['views'])
        # Exercise the file entrypoint with only the IDW present, under a new name.
        with tempfile.TemporaryDirectory(prefix='idw-only-') as tmp:
            isolated = Path(tmp)/'isolated.idw'; isolated.write_bytes((root/name).read_bytes())
            alone = read_drawing_file(isolated)
            checks.append(check('idw_only_same_display',
                display_signature(doc) == display_signature(alone)
                and [(i.reference,i.sha256) for i in doc.images] == [(i.reference,i.sha256) for i in alone.images]))
        failures = [dict(path=p,reason=v['reason']) for p,v in walk(native) if isinstance(v,dict) and v.get('status') == 'failed']
        results.append(dict(case=row['case'],source_sha256=doc.source_sha256,checks=checks,
            items=sum(len(s.items) for s in doc.sheets),omissions=[dict(o) for s in doc.sheets for o in s.omissions],
            getter_failures=failures,automatic_update=row['automatic_update'],native_before=row['before'],native_after=row['after'],
            images=[dict(reference=i.reference,width=i.width,height=i.height,sha256=i.sha256,status=i.status,diagnostic=i.diagnostic) for i in doc.images]))
    return dict(schema_version=1,status='bounded_checks_passed' if all(c['status']=='passed' for r in results for c in r['checks']) else 'failed',
        qualified=False,holdouts_used=False,scope='one_generated_regression_family_not_independent_holdouts',
        capture_sha256=hashlib.sha256((root/'annotations.native.json').read_bytes()).hexdigest(),
        manifest_sha256=hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),provider=capture['version'],
        unverified=['general_physical_units','current_state','glyph_shape_and_font_substitution','vector_view_semantics',
                    'general_clipping_and_draw_order','other_bitmap_layouts','independent_holdouts'],results=results)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input',type=Path,required=True);parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if args.output.exists():parser.error('Output must be a new report path')
    result=validate(args.input)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(result,ensure_ascii=False,allow_nan=False,indent=2)+'\n')
    print(json.dumps(dict(status=result['status'],cases=len(result['results']),checks=sum(len(r['checks']) for r in result['results']),qualified=False)))
    raise SystemExit(0 if result['status']=='bounded_checks_passed' else 1)


if __name__ == '__main__':main()
