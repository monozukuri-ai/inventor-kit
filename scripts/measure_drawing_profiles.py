"""Measure pinned regression IDWs against source-preserved Inventor API/PDF captures.

The collected session may migrate in memory and have unresolved external models.
This compares captured saved views; it does not qualify current state, complete
rendering, general units or unobserved record layouts. No fitted transform is used.
"""
import argparse
from collections import Counter
import hashlib
from itertools import permutations
import json
import math
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET

from drawing_corpus import drawing_rows

PT_TO_MM = 25.4 / 72


def identity(path):
    return dict(file=path.name, bytes=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest())


def require(value, message):
    if not value:
        raise ValueError(message)


def trace_paths(trace):
    # Some MuPDF versions emit unbalanced layer/group tags for these valid PDFs.
    # Every path already carries its absolute transform. Parse complete path
    # fragments only, check their counts, and never repair or infer transforms.
    pages = re.findall(r'<page\s+mediabox="([^"]+)"\s*>', trace)
    require(len(pages) == 1, 'Expected one PDF page')
    box = [float(x) for x in pages[0].split()]
    require(len(box) == 4 and all(map(math.isfinite, box)), 'Invalid page box')
    fragments = re.findall(r'<fill_path\b[^>]*>.*?</fill_path>', trace, re.S)
    require(len(fragments) == len(re.findall(r'<fill_path\b', trace)), 'Incomplete filled PDF paths')
    nodes = [ET.fromstring(s) for s in fragments]
    try:
        ET.fromstring(trace)
        envelope = 'well_formed'
    except ET.ParseError:
        envelope = 'unbalanced_container_tags_complete_path_fragments_used'
    return box, nodes, envelope


def polygon_mm(node, box):
    if [n.tag for n in node] != ['moveto', 'lineto', 'lineto', 'closepath']:
        return None
    matrix = [float(v) for v in node.attrib['transform'].split()]
    require(len(matrix) == 6 and all(map(math.isfinite, matrix)), 'Invalid PDF transform')
    a, b, c, d, e, f = matrix
    result = []
    for child in list(node)[:3]:
        x, y = float(child.attrib['x']), float(child.attrib['y'])
        require(math.isfinite(x) and math.isfinite(y), 'Nonfinite PDF point')
        result.append([(a*x+c*y+e-box[0])*PT_TO_MM, (box[3]-b*x-d*y-f)*PT_TO_MM])
    return result


def triangle_distance(a, b):
    return min(max(math.dist(p, q) for p, q in zip(a, ordered)) for ordered in permutations(b))


def point_segment_distance(p, a, b):
    direction = [y-x for x,y in zip(a,b)]
    length2 = sum(x*x for x in direction)
    t = 0 if length2 == 0 else min(1, max(0, sum((x-y)*v for x,y,v in zip(p,a,direction))/length2))
    return math.dist(p, [x+t*v for x,v in zip(a,direction)])


def colored_lines(sheet, trace, box):
    """Compare the explicit nonblack line subset, without moving/scaling to fit."""
    native = []
    for fragment in re.findall(r'<stroke_path\b[^>]*>.*?</stroke_path>', trace, re.S):
        node = ET.fromstring(fragment)
        if any(n.tag not in ('moveto', 'lineto', 'closepath') for n in node):
            continue
        color = [float(v) for v in node.attrib['color'].split()]
        if node.get('colorspace') == 'DeviceGray': color *= 3
        if len(color) != 3: continue
        a,b,c,d,e,f = map(float, node.attrib['transform'].split())
        previous = first = None
        for point in node:
            if point.tag == 'closepath': current = first
            else:
                x,y = float(point.attrib['x']),float(point.attrib['y'])
                current = [(a*x+c*y+e-box[0])*PT_TO_MM, (box[3]-b*x-d*y-f)*PT_TO_MM]
            if point.tag != 'moveto' and previous is not None and current is not None:
                native.append((previous, current, color))
            if point.tag == 'moveto': first = current
            previous = current
    errors, dashed_segments = [], 0
    for item in sheet.items:
        color = item.style['rgba']
        if item.geometry['kind'] != 'polyline' or color is None or max(color[:3])-min(color[:3]) < .1:
            continue
        points = item.geometry['points']
        if item.style['dash']:
            dashed_segments += len(points)-1
            continue
        for p,q in zip(points,points[1:]):
            a,b = [v*10 for v in p[:2]], [v*10 for v in q[:2]]
            candidates = [(c,d) for c,d,rgb in native if max(abs(x-y) for x,y in zip(color[:3],rgb)) < 1e-5]
            samples = [[x+(y-x)*i/24 for x,y in zip(a,b)] for i in range(25)]
            errors.append(max(min(point_segment_distance(p,c,d) for c,d in candidates) for p in samples) if candidates else None)
    return dict(saved_segments=len(errors), excluded_dashed_segments=dashed_segments,
                same_color_native_candidates_found=sum(e is not None for e in errors),
                comparison='25 samples per saved segment to same-color native line segments; no reverse coverage claim',
                max_sample_distance_mm=max((e for e in errors if e is not None),default=None), sample_distances_mm=errors)


def measure(root):
    from inventor_kit import read_drawing_file
    capture = json.loads((root/'profiles.native.json').read_text(encoding='utf-8-sig'))
    require(capture['format'] == 'inventor-kit-segment-profiles-native-v1', 'Unknown capture format')
    require(capture['save_requested'] is False and capture['update_requested'] is False, 'Capture requested mutation')
    require(capture['scope'] == 'original_idw_opened_deferred', 'Unexpected capture scope')
    script = identity(root/'capture.ps1')
    require(all(script[k] == capture['source_script'][k] for k in ('bytes', 'sha256')), 'Capture script identity mismatch')
    known = {row['sha256']: row for _, row in drawing_rows() if row['split'] == 'regression'}
    rows = []
    seen = set()
    priority = {24: 0, 29: 1, 28: 2, 26: 3}
    for row in sorted(capture['rows'], key=lambda r: priority[r['segment_major']]):
        source = row['source']
        require(source['file'] == Path(source['file']).name, 'Unsafe source path')
        require(source['sha256'] not in seen, 'Duplicate source capture')
        seen.add(source['sha256'])
        require(source['sha256'] in known, 'Capture is not a pinned regression input')
        require(source['file'] == Path(known[source['sha256']]['file']).name, 'Unexpected source filename')
        require(source == identity(root/source['file']) == row['source_after'], 'Captured source changed')
        require(source['bytes'] == known[source['sha256']]['bytes'], 'Input size differs from manifest')
        require(row['status'] == 'captured' and row['before'] == row['after'], 'Capture state changed or failed')
        require(row['before']['defer_updates'] is True, 'Deferred open was not observed')
        require(row['pdf']['status'] == 'captured', 'PDF not captured')
        pdf = row['pdf']['value']
        require(pdf['file'] == Path(pdf['file']).name and pdf == identity(root/pdf['file']), 'PDF identity mismatch')
        trace = subprocess.run(['mutool', 'draw', '-q', '-F', 'trace', str(root/pdf['file'])],
                               capture_output=True, text=True, check=True, timeout=60).stdout
        box, fills, envelope = trace_paths(trace)
        native_triangles = [p for node in fills if (p := polygon_mm(node, box)) is not None]
        doc = read_drawing_file(root/source['file'])
        require(len(doc.sheets) == len(row['sheets']) == 1, 'Expected one saved/captured sheet')
        sheet, native = doc.sheets[0], row['sheets'][0]
        paper_error = max(abs(a-b)*10 for a,b in zip(sheet.size_in_source_units,
                                                   (native['width_cm'], native['height_cm'])))
        actual_views = {v.name: v for v in sheet.views}
        require(len(actual_views) == len(sheet.views), 'Duplicate saved view name')
        view_checks = []
        for v in native['views']:
            saved = actual_views.get(v['name'])
            view_checks.append(dict(name=v['name'], matched=saved is not None,
                position_error_mm=None if saved is None else math.dist(
                    [saved.placement_transform[i][3]*10 for i in range(2)], [x*10 for x in v['position']])))
        saved_triangles = []
        for item in sheet.items:
            g = item.geometry
            if g['kind'] == 'triangles':
                for i in range(0, len(g['indices']), 3):
                    saved_triangles.append([[v*10 for v in g['vertices'][j][:2]] for j in g['indices'][i:i+3]])
        require(len(saved_triangles) <= 1024 and len(native_triangles) <= 1024, 'Triangle comparison budget exceeded')
        candidates = sorted((triangle_distance(a, b), i, j) for i,a in enumerate(saved_triangles)
                            for j,b in enumerate(native_triangles))
        matches, used_saved, used_native = [], set(), set()
        for distance, i, j in candidates:
            if i not in used_saved and j not in used_native:
                matches.append(dict(saved_index=i, native_index=j, max_vertex_error_mm=distance))
                used_saved.add(i); used_native.add(j)
        rows.append(dict(file=source['file'], segment_major=row['segment_major'], source=source, pdf=pdf,
            source_preserved=True, captured_state=row['before'], missing_references=sum(r['missing'] for r in row['references']),
            length_unit_api=row['length_unit'], paper_error_mm=paper_error,
            stored_name=sheet.name, native_name=native['name'], stored_view_count=len(sheet.views),
            native_view_count=len(native['views']), view_checks=view_checks,
            coordinate_scale=dict(mm_per_source_unit=10, scope='comparison_hypothesis_corroborated_for_these_paper_and_view_positions_only'),
            filled_triangles=dict(saved_count=len(saved_triangles), native_count=len(native_triangles),
                unmatched_saved_count=len(saved_triangles)-len(matches), unmatched_native_count=len(native_triangles)-len(matches),
                match_method='greedy_minimum_vertex_distance_without_fitted_transform', matches=matches,
                max_vertex_error_mm=max((m['max_vertex_error_mm'] for m in matches), default=None)),
            pdf_trace_envelope=envelope, display_items=len(sheet.items),
            colored_straight_lines=colored_lines(sheet, trace, box),
            omissions=dict(Counter(o['reason'] for o in sheet.omissions))))
    require(len(rows) == 5, 'Expected five profile captures')
    return dict(schema_version=1, status='measured', qualified=False, native_capture=identity(root/'profiles.native.json'),
        inventor_version=capture['version'], results=rows,
        limitations=['All captures have dirty=true; several require migration and have missing external models.',
            'Sources were preserved and before/after API state matched; current model state is not qualified.',
            'Paper/view positions and saved triangle vertices only; fonts, all curves, visibility and complete content are not qualified.',
            'PDF export rounding is reported as measured error, not used to fit source coordinates.'])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input', required=True, type=Path)
    p.add_argument('--output', required=True, type=Path)
    args = p.parse_args()
    if args.output.exists():
        p.error('Choose a new evidence path')
    result = measure(args.input.resolve())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x') as f:
        json.dump(result, f, indent=2, ensure_ascii=False, allow_nan=False)
        f.write('\n')
    for row in result['results']:
        print(json.dumps({k:row[k] for k in ('file','paper_error_mm','stored_view_count','native_view_count')} |
              dict(triangles=row['filled_triangles']['saved_count'], triangle_error_mm=row['filled_triangles']['max_vertex_error_mm'])))


if __name__ == '__main__':
    main()
