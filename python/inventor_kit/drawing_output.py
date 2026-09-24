"""Saved drawing reports and a shared, offline SVG renderer.

Coordinates are source units, never an implied millimeter/printing contract.
The Viewer mounts these same SVG elements and downloads the pristine document.
"""
from __future__ import annotations

import base64
from collections import Counter
from dataclasses import asdict
import hashlib
import json
import math
from pathlib import Path
import re
from xml.sax.saxutils import escape, quoteattr

from .drawing import DrawingDisplayError, _plain

MAX_SVG_BYTES = 32 * 1024 * 1024
MAX_REPORT_BYTES = 96 * 1024 * 1024
SVG_NS = 'http://www.w3.org/2000/svg'


def json_bytes(value, limit=MAX_REPORT_BYTES):
    chunks, count = [], 0
    for chunk in json.JSONEncoder(ensure_ascii=False, allow_nan=False, separators=(',', ':')).iterencode(value):
        data = chunk.encode('utf-8')
        count += len(data)
        if count > limit:
            raise ValueError('Drawing JSON byte limit exceeded')
        chunks.append(data)
    return b''.join(chunks)


def selected_sheet(doc, sheet_id):
    if sheet_id is None:
        if len(doc.sheets) != 1:
            raise DrawingDisplayError(None, [dict(code='drawing.sheet_required',
                message='Select one sheet ID from the drawing report.')])
        return doc.sheets[0]
    try:
        return doc.sheet(sheet_id)
    except KeyError:
        raise DrawingDisplayError(sheet_id, [dict(code='drawing.invalid_sheet_id',
            message='The sheet ID does not belong to this input.')]) from None


def item_dict(item):
    return dict(id=item.id, geometry=_plain(item.geometry), style=_plain(item.style),
        source=asdict(item.source), segment_id=item.segment_id, record_ordinal=item.record_ordinal,
        placement_record=item.placement_record, group_path=list(item.group_path))


def drawing_report(doc, *, sheet_id=None, details=False, list_only=False):
    """JSON-compatible inventory, including unresolved content and raw text runs."""
    if type(details) is not bool or type(list_only) is not bool:
        raise TypeError('details and list_only must be bool')
    sheets = doc.sheets if sheet_id is None else (selected_sheet(doc, sheet_id),)
    rows = []
    for sheet in sheets:
        row = dict(id=sheet.id, index=sheet.index, name=sheet.name, status=sheet.status,
            size_in_source_units=sheet.size_in_source_units, content_coverage=sheet.content_coverage,
            item_count=len(sheet.items), view_count=len(sheet.views), omission_count=len(sheet.omissions),
            geometry_kinds=dict(Counter(i.geometry['kind'] for i in sheet.items)), diagnostics=list(sheet.diagnostics))
        if not list_only:
            row.update(views=[asdict(v) for v in sheet.views], omissions=[_plain(o) for o in sheet.omissions],
                sources=[asdict(s) for s in sheet.sources],
                text_runs=[item_dict(i) for i in sheet.items if i.geometry['kind'] == 'text'],
                unresolved_styles=dict(Counter(reason for i in sheet.items for reason in i.style.get('unresolved', ()))))
        if details:
            row['items'] = [item_dict(i) for i in sheet.items]
        rows.append(row)
    return _plain(dict(schema_version=1, report_type='drawing', kind='drawing',
        status='partial' if any(s.status != 'unavailable' for s in sheets) else 'unsupported',
        source_sha256=doc.source_sha256, units=doc.units, length_unit=doc.length_unit,
        millimeters_per_unit=doc.millimeters_per_unit, qualified=doc.qualified, complete=doc.complete,
        current_state=doc.current_state, current_state_verified=False, snapshot_kind=doc.snapshot_kind,
        reference_freshness=doc.reference_freshness, sheet_status=doc.sheet_status,
        sheet_count=len(doc.sheets), selected_sheet_id=sheet_id, sheets=rows,
        images=[dict(reference=i.reference, status=i.status, mime_type=i.mime_type, width=i.width,
            height=i.height, sha256=i.sha256, source=asdict(i.source), diagnostic=i.diagnostic) for i in doc.images],
        diagnostics=[_plain(d) for d in doc.diagnostics]))


def _xml(value):
    # XML 1.0 cannot carry NUL and certain controls. Preserve the original in
    # the JSON report, and expose the replacement on the displayed element.
    return ''.join(c if c in '\t\n\r' or '\x20' <= c <= '\ud7ff'
                   or '\ue000' <= c <= '\ufffd' or '\U00010000' <= c <= '\U0010ffff'
                   else '\ufffd' for c in str(value))


def _number(value):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError('Non-finite SVG coordinate or style')
    return format(value, '.15g')


def _family(name):
    # One literal family, never an injected list or an external font resource.
    name = re.sub(r'[\x00-\x1f\x7f"\\]', lambda m: '\\' + format(ord(m[0]), 'x') + ' ', name)
    return '"' + name + '"'


def text_presentation(g):
    font = g.get('font')
    symbol = bool(font and g['text'] in ('n', 'x') and g['raw_flags'] == 9
                  and font['family'].lower() == 'aigdt' and font['flags'] == 0 and font['weight_candidate'] == 400)
    family = font['family'] if font else ''
    cjk = bool(re.search(r'[\u3000-\u30ff\u3400-\u9fff\uf900-\ufaff\uff00-\uffef]', g['text']))
    fallbacks = ['Noto Sans CJK JP', 'Yu Gothic', 'Meiryo', 'Hiragino Sans',
                 'Hiragino Kaku Gothic ProN', 'IPAGothic', 'IPAPGothic'] if cjk else []
    # Explicit symbol fonts avoid a legacy AIGDT glyph being used for Unicode.
    if symbol:
        fallbacks = ['DejaVu Sans', 'Segoe UI Symbol', 'Noto Sans Symbols 2']
    names = ([] if symbol or not family else [family]) + fallbacks
    css = ', '.join([_family(n) for n in dict.fromkeys(names)] + ['sans-serif'])
    # Major24 Template_IACS: native PDF glyph heights confirm cap-height
    # sizing. Vafle families use Arial in that PDF; local shape remains a
    # font fallback, not an assertion that the original font is installed.
    measured_styles = {
        'arial': ((0, 400),), 'tahoma': ((0, 400), (0, 700), (1, 400)),
        'isocp': ((0, 400),), 'isocp_iv25': ((0, 400), (0, 700)),
        'vafle vut': ((0, 400),), 'vafle light vut': ((0, 400),),
    }
    measured = bool(font and (symbol or (font['flags'], font['weight_candidate'])
                             in measured_styles.get(family.lower(), ())))
    capital = bool(measured and g['raw_flags'] == 9 and font['height_candidate'] > 0
                   and not re.search(r'[\r\n]', g['text']))
    display = {'n': '⌀', 'x': '↧'}[g['text']] if symbol else g['text']
    return dict(text=display, family=css, capital=capital, symbol=symbol, cjk=cjk)


def _curve(g, height):
    """Exact affine ellipse arcs, including reflected/non-orthogonal axes.

    Eigenvalues of A A^T give SVG principal radii; the determinant gives sweep.
    No sampling error or polygon corners are introduced into dashed curves.
    """
    cx, cy = g['center'][:2]
    ux, uy = g['u'][:2]
    vx, vy = g['v'][:2]
    a, b, c, d = ux, vx, -uy, -vy
    determinant = a * d - b * c
    xx, xy, yy = a*a+b*b, a*c+b*d, c*c+d*d
    major2 = (xx + yy + math.hypot(xx - yy, 2*xy)) / 2
    major = math.sqrt(major2)
    minor = abs(determinant) / major if major else 0
    angle = math.degrees(math.atan2(2*xy, xx-yy) / 2)
    start, end = g['start'], g['end']
    span = end - start
    if not math.isfinite(span) or not 0 < span <= 2*math.pi + 1e-8:
        raise ValueError('Unsupported saved drawing arc interval')
    def point(t):
        return f'{_number(cx+ux*math.cos(t)+vx*math.sin(t))} {_number(height-cy-uy*math.cos(t)-vy*math.sin(t))}'
    parts = ['M ' + point(start)]
    # Edge-on projected circles have almost collinear XY axes (observed minor
    # radii down to 1e-18). SVG endpoint-to-center recovery is ill-conditioned
    # there and browsers can draw billion-unit arcs. Retain endpoints and every
    # coordinate extremum as straight segments. Their analytic deviation is at
    # most twice the minor radius; do this only below 2e-9 source units. This
    # bound excludes numeric serialization/browser rasterization error.
    if minor <= min(1e-9, 1e-6 * major):
        extrema = []
        theta = math.radians(angle)
        # Principal-axis extrema guarantee monotonic major coordinates in
        # each interval, which makes the 2*minor chord error bound applicable.
        principal = (a*math.cos(theta)+c*math.sin(theta), b*math.cos(theta)+d*math.sin(theta))
        for x, y in ((ux, vx), (uy, vy), principal):
            phase = math.atan2(y, x)
            first = math.floor((start - phase) / math.pi) + 1
            for k in range(first, first + 3):
                t = phase + k * math.pi
                if start < t < end:
                    extrema.append(t)
        for t in sorted(set(extrema)) + [end]:
            parts.append('L ' + point(t))
        return ' '.join(parts), 2 * minor
    # Keep endpoint-to-center recovery away from the ill-conditioned diameter
    # case too: quarter arcs avoid sqrt(float-rounding-error) center shifts.
    steps = max(1, math.ceil(span / (math.pi / 2)))
    for i in range(1, steps+1):
        parts.append(f'A {_number(major)} {_number(minor)} {_number(angle)} 0 {int(determinant > 0)} ' + point(start+span*i/steps))
    return ' '.join(parts), None


def _straight_dash(g, dash, major):
    """Observed straight-line fitting, leaving the nominal source style intact.

    Major31 controls establish the standard patterns on long lines. The
    preserved major26/28/29 PDFs establish hidden lines, including the short
    thirds rule; major24 establishes only short projection-symbol lines.
    Curves, sampled splines and unobserved patterns keep nominal dashes.
    """
    if not dash or g['kind'] != 'polyline' or len(g['points']) != 2:
        return None
    patterns = (
        (12, 3), (12, 12), (24, 3, .5, 3), (24, 3, .5, 3, .5, 3),
        (24, 3, .5, 3, .5, 3, .5, 3), (.5, 3), (24, 3, 6, 3),
        (24, 3, 6, 3, 6, 3), (12, 3, .5, 3, .5, 3), (12, 3, .5, 3),
        (12, 3, 12, 3, .5, 3), (12, 3, 12, 3, .5, 3, .5, 3),
        (12, 3, .5, 3, .5, 3, .5, 3), (12, 3, 12, 3, .5, 3, .5, 3, .5, 3),
    ) if major == 31 else (((12, 3),) if major in (26, 28, 29)
                           else (((5, 1, 1, 1),) if major == 24 else ()))
    if not any(len(p) == len(dash) and all(math.isclose(v / dash[0], x / p[0], rel_tol=1e-7)
                                         for v, x in zip(dash, p)) for p in patterns):
        return None
    length = math.dist(g['points'][0][:2], g['points'][1][:2])
    period = sum(dash)
    if not math.isfinite(length) or not math.isfinite(period):
        raise ValueError('Non-finite drawing dash fitting')
    if length == 0:
        return None
    if length < period:
        # No short major31 controls were captured. Do not infer that branch
        # from the older profiles or from long-line observations alone.
        if major not in (24, 26, 28, 29) or length / 3 == 0:
            return None
        return [length / 3, length / 3], 0., 'short-line-thirds'
    if major == 24:
        return None
    ratio = length / period
    # Do not add fitting for extreme repetition counts or numerical boundaries.
    # Half-period ties have not been observed; keep them nominal.
    if not math.isfinite(ratio) or ratio * len(dash) >= 100000:
        return None
    if abs(ratio - math.floor(ratio) - .5) < 1e-10:
        return None
    periods = max(1, round(ratio))
    if periods * len(dash) >= 100000:
        return None
    scale = (length / periods) / period
    fitted = [v * scale for v in dash]
    if any(v <= 0 or not math.isfinite(v) for v in fitted) or fitted[0] / 2 == 0:
        return None
    return fitted, fitted[0] / 2, 'whole-period-half-dash-ends'


def render_svg(sheet, source_sha256, images, *, max_bytes=MAX_SVG_BYTES):
    """Render a plain source-sheet payload; used by both public API and Viewer.

    images maps native reference numbers to {mime_type, data, sha256}. Only
    verified PNG/JPEG bytes are embedded. No script, external URL or font fetch.
    """
    if type(max_bytes) is not int or not 0 < max_bytes <= MAX_SVG_BYTES:
        raise ValueError('max_bytes must be a positive integer at most 32 MiB')
    size = sheet.get('size_in_source_units')
    if sheet.get('status') == 'unavailable' or not size or len(size) != 2 or any(v <= 0 for v in size):
        raise DrawingDisplayError(sheet['id'], [dict(code='drawing.sheet_unavailable', message='Stored sheet display is unavailable.')])
    width, height = size
    chunks, count = [], 0
    def put(text):
        nonlocal count
        count += len(text.encode('utf-8'))
        if count > max_bytes:
            raise ValueError('Drawing SVG byte limit exceeded')
        chunks.append(text)
    def node(tag, attrs, text=None, close=True):
        attributes = ' '.join(k+'='+quoteattr(_xml(v)) for k,v in attrs.items())
        if text is not None:
            put(f'<{tag} {attributes}>' + escape(_xml(text)) + f'</{tag}>')
        else:
            put(f'<{tag} {attributes}' + ('/>' if close else '>'))
    put('<?xml version="1.0" encoding="UTF-8"?>\n')
    node('svg', dict(xmlns=SVG_NS, viewBox=f'0 0 {_number(width)} {_number(height)}',
        width='1200', height=_number(1200*height/width), role='img',
        **{'aria-label': 'Saved drawing · Partial display · Source units unverified',
           'data-source-sha256': source_sha256, 'data-sheet-id': sheet['id'], 'data-renderer-version': '1'}), close=False)
    node('title', {}, sheet.get('name') or 'Saved sheet')
    node('desc', {}, 'Saved partial drawing. Physical units, fonts and current state are unverified. See the accompanying JSON report.')
    node('metadata', {}, json.dumps(dict(source_sha256=source_sha256, sheet_id=sheet['id'],
        units='source_units_unverified', complete=False, qualified=False, snapshot_kind='saved',
        reference_freshness='unverified'), ensure_ascii=False))
    node('rect', dict(x='0', y='0', width=_number(width), height=_number(height), fill='white',
        stroke='#b8c4cf', **{'stroke-width': '.035'}))
    used = {i['geometry']['reference'] for i in sheet['items'] if i['geometry']['kind'] == 'image'}
    present = set()
    put('<defs>')
    for reference in sorted(used):
        image = images.get(reference)
        if not image or image.get('data') is None:
            continue
        data, mime = image['data'], image['mime_type']
        if mime not in ('image/png', 'image/jpeg') or hashlib.sha256(data).hexdigest() != image['sha256']:
            raise ValueError('Drawing image identity/type mismatch')
        if len(data)*4//3 > max_bytes-count:
            raise ValueError('Drawing SVG image byte limit exceeded')
        node('image', dict(id=f'drawing-image-{reference}', href=f'data:{mime};base64,'+base64.b64encode(data).decode('ascii'),
            width='1', height='1', preserveAspectRatio='none'))
        present.add(reference)
    put('</defs>')
    for item in sheet['items']:
        g, style = item['geometry'], item['style']
        rgba = style.get('rgba')
        color = '#111'
        if rgba is not None:
            if len(rgba) != 4 or any(not math.isfinite(c) or not 0 <= c <= 1 for c in rgba):
                raise ValueError('Invalid drawing color')
            color = f'rgb({",".join(str(math.floor(c*255+.5)) for c in rgba[:3])})'
        attrs = {'data-item-id': item['id'], 'data-kind': g['kind']}
        if rgba is not None:
            attrs['opacity'] = _number(rgba[3])
        if style.get('unresolved'):
            attrs['data-style-state'] = 'unverified'
        if g['kind'] == 'image':
            if g['reference'] not in present:
                continue
            attrs.update(href=f"#drawing-image-{g['reference']}", transform='matrix('+ ' '.join(_number(v) for v in
                (g['u'][0], -g['u'][1], g['v'][0], -g['v'][1], g['origin'][0], height-g['origin'][1]))+')')
            node('use', attrs)
        elif g['kind'] == 'text':
            f, p = g.get('font'), text_presentation(g)
            factor = f.get('width_factor') if f else None
            factor = 1 if factor is None else factor
            d, u = g['direction'], g['up']
            attrs.update(transform='matrix('+' '.join(_number(v) for v in
                (d[0]*factor,-d[1]*factor,-u[0],u[1],g['position'][0],height-g['position'][1]))+')', fill=color)
            attrs.update({'font-family': p['family'], 'font-size': _number(f['height_candidate'] if f else .25),
                'font-weight': str(f['weight_candidate'] if f else 400), 'font-style': 'italic' if f and f['flags']==1 else 'normal',
                'style': 'white-space:pre;'+('font-size-adjust:cap-height 1' if p['capital'] else 'font-size-adjust:none'),
                'data-font-sizing': 'capital-height-candidate' if p['capital'] else 'unverified-em-fallback'})
            if p['symbol']:
                attrs.update({'data-symbol-fallback':f"AIGDT:{g['text']}->U+{ord(p['text']):04X}",'data-raw-text':g['text']})
            if p['cjk']:
                attrs['data-font-fallback'] = 'system-cjk'
            if _xml(g['text']) != g['text']:
                attrs['data-text-replacement'] = 'xml-1.0-control'
            node('text', attrs, close=False)
            for index,line in enumerate(re.split(r'\r\n|\n|\r', p['text'])):
                node('tspan', dict(x='0', dy='1.2em' if index else '0'), line)
            put('</text>')
        elif g['kind'] == 'triangles':
            vertices, indices = g['vertices'], g['indices']
            if len(vertices) not in (3, 6) or len(indices) != len(vertices) or any(
                    type(i) is not int or not 0 <= i < len(vertices) for i in indices):
                raise ValueError('Invalid saved triangle topology')
            if any(len(p) != 3 or any(not math.isfinite(v) for v in p) for p in vertices):
                raise ValueError('Invalid saved triangle vertices')
            paths = []
            for offset in range(0, len(indices), 3):
                points = [vertices[i] for i in indices[offset:offset + 3]]
                a, b, c = points
                area = (b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0])
                if not math.isfinite(area) or area == 0 or a[2] != b[2] or b[2] != c[2]:
                    raise ValueError('Degenerate or nonplanar saved triangle')
                paths.append('M'+' L'.join(_number(p[0])+','+_number(height-p[1]) for p in points)+' Z')
            attrs.update(fill=color, stroke='none', d=' '.join(paths))
            node('path', attrs)
        elif g['kind'] in ('polyline','curve'):
            stroke = style.get('width')
            if stroke is not None and stroke <= 0:
                raise ValueError('Invalid drawing stroke width')
            dash = style.get('dash') or []
            if any(isinstance(v, bool) or not isinstance(v, (int, float))
                   or not math.isfinite(v) or v <= 0 for v in dash) or len(dash)%2:
                raise ValueError('Invalid drawing dash sequence')
            if dash:
                fitted = _straight_dash(g, dash, sheet.get('segment_majors', {}).get(item.get('segment_id')))
                attrs['data-dash-rendering'] = 'nominal-unverified'
                if fitted is not None:
                    dash, offset, mode = fitted
                    attrs.update({'stroke-dashoffset': _number(offset), 'data-dash-rendering': mode})
            attrs.update(fill='none', stroke=color, **{'stroke-width':_number(.035 if stroke is None else stroke),
                'stroke-dasharray':' '.join(_number(v) for v in dash) or 'none', 'stroke-linejoin':'round', 'stroke-linecap':'butt'})
            if g['kind']=='curve':
                attrs['d'], projection_error = _curve(g, height)
                if g.get('filled') is True:
                    # The saved arc closes by a chord. A sector's separate
                    # triangle remains its own independently owned item.
                    attrs.update(fill=color, stroke='none', d=attrs['d']+' Z')
                if projection_error is not None:
                    attrs['data-curve-rendering'] = 'near-degenerate-line-projection'
                    attrs['data-projection-error-bound'] = _number(projection_error)
                node('path',attrs)
            else:
                attrs['points'] = ' '.join(_number(p[0])+','+_number(height-p[1]) for p in g['points'])
                node('polyline',attrs)
        else:
            raise ValueError('Unsupported saved drawing element')
    put('</svg>\n')
    return ''.join(chunks)


def source_sheet(doc, sheet):
    return dict(id=sheet.id, name=sheet.name, status=sheet.status, size_in_source_units=sheet.size_in_source_units,
        segment_majors={s.id: s.major for s in doc.metadata.segments},
        items=[item_dict(i) for i in sheet.items])


def svg_document(doc, *, sheet_id=None, allow_partial=False, max_bytes=MAX_SVG_BYTES):
    if type(allow_partial) is not bool:
        raise TypeError('allow_partial must be bool')
    sheet = selected_sheet(doc, sheet_id)
    if not allow_partial:
        raise DrawingDisplayError(sheet.id, [dict(code='drawing.partial_permission_required',
            message='Saved SVG display is partial; explicitly set allow_partial=True.')], sheet.omissions)
    return render_svg(source_sheet(doc,sheet), doc.source_sha256,
        {i.reference:dict(mime_type=i.mime_type,data=i.data,sha256=i.sha256) for i in doc.images},max_bytes=max_bytes)


def export_svg(doc, path, *, sheet_id=None, allow_partial=False, max_bytes=MAX_SVG_BYTES):
    path=Path(path)
    if path.suffix.lower() != '.svg':
        raise ValueError('SVG output must use .svg')
    sidecar=path.with_suffix(path.suffix+'.json')
    if path.exists() or sidecar.exists():
        raise FileExistsError('SVG and its JSON sidecar must be new files')
    sheet=selected_sheet(doc,sheet_id)
    data=svg_document(doc,sheet_id=sheet.id,allow_partial=allow_partial,max_bytes=max_bytes).encode('utf-8')
    report=svg_report(doc, sheet, data, str(path))
    encoded=json_bytes(report)
    created=[]
    try:
        for target,body in ((path,data),(sidecar,encoded)):
            with target.open('xb') as stream:
                created.append(target)
                stream.write(body)
    except BaseException:
        for target in created:
            target.unlink()
        raise
    return report


def svg_report(doc, sheet, data, path=None):
    report=drawing_report(doc,sheet_id=sheet.id)
    report['export']=dict(format='svg',path=path,sha256=hashlib.sha256(data).hexdigest(),bytes=len(data),
        renderer_version=1,physical_scale_verified=False,fonts='local_system_fallback',
        curves='affine_ellipse_arcs_and_saved_polylines',images='embedded_saved_pixels',
        near_degenerate_curves=dict(count=data.count(b'data-curve-rendering="near-degenerate-line-projection"'),
            method='line segments through endpoints and coordinate extrema',
            analytic_error_bound_source_units=2e-9, excludes='serialization and browser rasterization error'),
        dash_rendering=dict(
            whole_period_lines=data.count(b'data-dash-rendering="whole-period-half-dash-ends"'),
            short_lines=data.count(b'data-dash-rendering="short-line-thirds"'),
            nominal_unverified=data.count(b'data-dash-rendering="nominal-unverified"'),
            qualification='observed profiles only; nominal source styles and unresolved flags retained'),
        text_replacement='XML 1.0 controls become U+FFFD; original text remains in text_runs')
    return report
