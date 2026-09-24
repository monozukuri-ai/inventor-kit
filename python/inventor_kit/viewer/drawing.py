"""Publish input-bound sheets only after the complete worker succeeds."""
from dataclasses import asdict
import hashlib
import json
import re

from ..drawing import read_drawing, _plain
from ..drawing_output import item_dict, source_sheet, render_svg, svg_report

MAX_SHEET_BYTES = 32 * 1024 * 1024
MAX_DRAWING_BYTES = 128 * 1024 * 1024


def _pack_item_records(payload):
    """Wire v4 shares placement/source bases and derives canonical item IDs."""
    placements, bases, placement_indices, base_indices, items = [], [], {}, {}, []
    for item in payload['items']:
        if set(item) != {'id', 'geometry', 'source', 'segment_id', 'record_ordinal',
                         'placement_record', 'group_path', 'style_index'}:
            raise ValueError('Unexpected drawing item fields')
        source = item['source']
        if set(source) != {'source_id', 'stream', 'byte_domain', 'start_offset', 'end_offset'}:
            raise ValueError('Unexpected drawing source fields')
        identity = f"{payload['sheet_id']}/{item['segment_id']}/{item['placement_record']}/{item['record_ordinal']}"
        if item['id'] != identity:
            raise ValueError('Noncanonical drawing item ID')
        placement = (item['segment_id'], item['placement_record'], tuple(item['group_path']))
        base = (source['source_id'], source['stream'], source['byte_domain'])
        if placement not in placement_indices:
            placement_indices[placement] = len(placements)
            placements.append(list(placement))
        if base not in base_indices:
            base_indices[base] = len(bases)
            bases.append(list(base))
        items.append(dict(geometry=item['geometry'], style_index=item['style_index'],
            record=[placement_indices[placement], item['record_ordinal'], base_indices[base],
                    source['start_offset'], source['end_offset']]))
    return dict(payload, schema_version=4, items=items, placements=placements, source_bases=bases)


def _unpack_item_records(payload):
    items, placements, bases = (payload.get(k) for k in ('items', 'placements', 'source_bases'))
    sheet = payload.get('sheet_id')
    if (not isinstance(sheet, str) or len(sheet) > 512 or not isinstance(items, list) or len(items) > 100000
            or not isinstance(placements, list) or len(placements) > len(items)
            or not isinstance(bases, list) or len(bases) > len(items)):
        raise ValueError('Invalid drawing item record tables')
    def integer(n):
        return type(n) is int and 0 <= n <= 9007199254740991
    for p in placements:
        if (not isinstance(p, (list, tuple)) or len(p) != 3 or not isinstance(p[0], str)
                or not re.fullmatch(r'[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}', p[0])
                or not integer(p[1]) or not isinstance(p[2], (list, tuple)) or len(p[2]) > 128
                or any(not integer(n) for n in p[2])):
            raise ValueError('Invalid drawing placement record')
    for base in bases:
        if (not isinstance(base, (list, tuple)) or len(base) != 3 or any(not isinstance(s, str) for s in base)
                or base[2] not in ('cfb_stream', 'inflated_stream')):
            raise ValueError('Invalid drawing source base')
    expanded, identities = [], set()
    for item in items:
        if not isinstance(item, dict) or set(item) != {'geometry', 'style_index', 'record'}:
            raise ValueError('Invalid drawing compact item')
        r = item['record']
        if (not isinstance(r, (list, tuple)) or len(r) != 5 or any(not integer(n) for n in r)
                or r[0] >= len(placements) or r[2] >= len(bases) or r[3] > r[4]):
            raise ValueError('Invalid drawing compact item reference')
        segment, placement, path = placements[r[0]]
        source, stream, domain = bases[r[2]]
        identity = f'{sheet}/{segment}/{placement}/{r[1]}'
        if identity in identities:
            raise ValueError('Duplicate drawing item ID')
        identities.add(identity)
        expanded.append(dict(id=identity, geometry=item['geometry'], style_index=item['style_index'],
            segment_id=segment, record_ordinal=r[1], placement_record=placement, group_path=path,
            source=dict(source_id=source, stream=stream, byte_domain=domain, start_offset=r[3], end_offset=r[4])))
    return dict({k: v for k, v in payload.items() if k not in ('placements', 'source_bases')},
                schema_version=3, items=expanded)


def _pack_styles(payload):
    """Wire v2 shares identical styles including their complete source evidence.

    Expansion reuses the same style objects. Neither the resource byte bound
    nor any geometry, omission, source span or exported report is changed.
    """
    indices, styles, items = {}, [], []
    for item in payload['items']:
        key = json.dumps(item['style'], ensure_ascii=False, allow_nan=False, sort_keys=True, separators=(',', ':'))
        if key not in indices:
            indices[key] = len(styles)
            styles.append(item['style'])
        items.append(dict((k, v) for k, v in item.items() if k != 'style'))
        items[-1]['style_index'] = indices[key]
    return dict(payload, schema_version=2, styles=styles, items=items)


def _unpack_styles(payload):
    if not isinstance(payload, dict) or type(payload.get('schema_version')) is not int:
        raise ValueError('Unsupported drawing sheet schema')
    if payload.get('schema_version') == 4:
        payload = _unpack_item_records(payload)
    if payload.get('schema_version') == 1:
        return payload
    version = payload.get('schema_version')
    if version not in (2, 3):
        raise ValueError('Unsupported drawing sheet schema')
    styles, items = payload.get('styles'), payload.get('items')
    if (not isinstance(styles, list) or not isinstance(items, list) or len(styles) > len(items) or len(items) > 100000
            or any(not isinstance(s, dict) for s in styles)
            or any(not isinstance(i, dict) or 'style' in i or type(i.get('style_index')) is not int
                   or not 0 <= i['style_index'] < len(styles) for i in items)):
        raise ValueError('Invalid drawing style table or reference')
    result = dict(payload, schema_version=1,
        items=[dict({k: v for k, v in i.items() if k != 'style_index'}, style=styles[i['style_index']]) for i in items])
    del result['styles']
    if version == 3:
        ids = [i['id'] for i in result['items']]
        def views(rows, budget):
            if not isinstance(rows, list) or len(rows) > 4096:
                raise ValueError('Invalid drawing view reference table')
            expanded = []
            for view in rows:
                refs = view.get('item_indices') if isinstance(view, dict) else None
                if (not isinstance(refs, list) or 'item_ids' in view or len(refs) > budget[0]
                        or any(type(i) is not int or not 0 <= i < len(ids) for i in refs)):
                    raise ValueError('Invalid drawing view item reference')
                budget[0] -= len(refs)
                expanded.append(dict({k: v for k, v in view.items() if k != 'item_indices'},
                                     item_ids=[ids[i] for i in refs]))
            return expanded
        result['views'] = views(result.get('views'), [100000])
        report = result.get('export_report')
        sheets = report.get('sheets') if isinstance(report, dict) else None
        if not isinstance(sheets, list) or len(sheets) > 256 or any(not isinstance(s, dict) for s in sheets):
            raise ValueError('Invalid drawing export view reference table')
        budget = [100000]
        result['export_report'] = dict(report, sheets=[dict(s, views=views(s.get('views'), budget)) for s in sheets])
    return result


def _pack_view_references(payload):
    """Wire v3 replaces repeated view member IDs with bounded item indices."""
    indices = {item['id']: n for n, item in enumerate(payload['items'])}
    if len(indices) != len(payload['items']):
        raise ValueError('Duplicate drawing item ID')
    def views(rows):
        return [dict({k: v for k, v in view.items() if k != 'item_ids'},
                     item_indices=[indices[i] for i in view['item_ids']]) for view in rows]
    report = payload['export_report']
    return dict(payload, schema_version=3, views=views(payload['views']),
        export_report=dict(report, sheets=[dict(s, views=views(s['views'])) for s in report['sheets']]))


def validate_drawing_resources(directory, scene, limit=MAX_DRAWING_BYTES):
    """Verify committed descriptors and files before making them HTTP resources."""
    drawing = scene.get('drawing')
    if drawing is None:
        return
    source = scene['source']['sha256']
    if scene.get('scene_kind') != 'drawing' or source != drawing['source_sha256']:
        raise ValueError('Drawing scene source identity mismatch')
    total, ids = 0, set()
    for sheet in drawing['sheets']:
        if sheet['id'] in ids or not sheet['id'].startswith(source + '/'):
            raise ValueError('Duplicate or foreign drawing sheet')
        ids.add(sheet['id'])
        name = sheet['resource']
        if name is None:
            if sheet['status'] != 'unavailable' or sheet['bytes'] is not None or sheet['sha256'] is not None:
                raise ValueError('Unpublished sheet has inconsistent descriptors')
            continue
        if (sheet['status'] == 'unavailable' or not re.fullmatch(r'drawing-sheet-[a-f0-9]{64}\.json', name)
                or name != f"drawing-sheet-{sheet['sha256']}.json"):
            raise ValueError('Invalid drawing sheet resource')
        path = directory / name
        if path.is_symlink() or not 0 < sheet['bytes'] <= min(MAX_SHEET_BYTES, limit - total):
            raise ValueError('Drawing sheet resource limit exceeded')
        with path.open('rb') as stream:
            body = stream.read(sheet['bytes'] + 1)
        if len(body) != sheet['bytes'] or hashlib.sha256(body).hexdigest() != sheet['sha256']:
            raise ValueError('Drawing sheet resource integrity mismatch')
        payload = _unpack_styles(json.loads(body))
        if (payload['schema_version'] != 1 or payload['scene_kind'] != 'drawing_sheet'
                or payload['source_sha256'] != source or payload['sheet_id'] != sheet['id']
                or payload['units'] != drawing['units'] or len(payload['items']) != sheet['item_count']
                or len(payload['omissions']) != sheet['omission_count']):
            raise ValueError('Drawing sheet payload identity/count mismatch')
        svg = payload['svg'].encode('utf-8')
        report = payload['export_report']
        if (report['source_sha256'] != source or report['selected_sheet_id'] != sheet['id']
                or report['export']['sha256'] != hashlib.sha256(svg).hexdigest()
                or report['export']['bytes'] != len(svg)):
            raise ValueError('Drawing SVG report identity mismatch')
        item_ids = [i['id'] for i in payload['items']]
        item_set = set(item_ids)
        if len(item_set) != len(item_ids) or any(not i.startswith(sheet['id'] + '/') for i in item_ids):
            raise ValueError('Duplicate or foreign drawing item')
        view_ids = [v['id'] for v in payload.get('views', [])]
        if len(view_ids) != len(set(view_ids)) or any(not v.startswith(sheet['id'] + '/view-') for v in view_ids):
            raise ValueError('Duplicate or foreign drawing view')
        for view in payload.get('views', []):
            if any(i not in item_set for i in view['item_ids']):
                raise ValueError('Drawing view references an unknown item')
        total += len(body)
    for image in drawing['images']:
        name = image['resource']
        if name is None:
            continue
        if not re.fullmatch(r'drawing-image-[0-9]+\.(png|jpg)', name):
            raise ValueError('Invalid drawing image resource')
        path = directory / name
        if path.is_symlink() or path.stat().st_size > limit - total:
            raise ValueError('Drawing image resource limit exceeded')
        body = path.read_bytes()
        if len(body) > limit - total or hashlib.sha256(body).hexdigest() != image['sha256']:
            raise ValueError('Drawing image resource integrity mismatch')
        total += len(body)


def sheet_bytes(payload, limit=MAX_SHEET_BYTES):
    """Bound serialized output while encoding, before concatenating it."""
    chunks, total = [], 0
    for chunk in json.JSONEncoder(ensure_ascii=False, allow_nan=False, separators=(',', ':')).iterencode(payload):
        data = chunk.encode('utf-8')
        total += len(data)
        if total > limit:
            raise ValueError('Drawing sheet JSON byte limit exceeded')
        chunks.append(data)
    return b''.join(chunks)


def build_drawing_scene(data, directory, options, scene):
    scene['units'] = 'source_units_unverified'
    scene['scene_kind'] = 'drawing'
    if (options.candidate_id or options.require_current_state or options.body_ids or
            options.search_roots or options.allow_unverified_state):
        raise ValueError('IPT/IAM selection options do not apply to IDW drawings')
    doc = read_drawing(data, source_id=scene['source']['name'], limits=options.limits, drawing_limits=options.drawing_limits)
    scene['diagnostics'].extend(_plain(d) for d in doc.diagnostics)
    sheets, needed = [], set()
    total, limit = 0, min(options.max_buffer_bytes, MAX_DRAWING_BYTES)
    for sheet in doc.sheets:
        issues = list(sheet.diagnostics)
        # The Viewer presents saved source coordinates. render_sheet() instead
        # requests a physically scaled display and intentionally refuses unknown
        # units; it is not the admission check for this partial saved display.
        publish = sheet.status != 'unavailable'
        descriptor = dict(id=sheet.id, index=sheet.index, name=sheet.name,
            status=sheet.status if publish else 'unavailable',
            size_in_source_units=list(sheet.size_in_source_units) if sheet.size_in_source_units is not None else None,
            content_coverage=sheet.content_coverage, appearance='unverified',
            item_count=len(sheet.items), omission_count=len(sheet.omissions),
            diagnostics=issues, resource=None, bytes=None, sha256=None)
        if publish:
            items = [item_dict(i) for i in sheet.items]
            svg = render_svg(source_sheet(doc, sheet), doc.source_sha256,
                {i.reference: dict(mime_type=i.mime_type,data=i.data,sha256=i.sha256) for i in doc.images})
            payload = dict(schema_version=1, scene_kind='drawing_sheet', source_sha256=doc.source_sha256,
                sheet_id=sheet.id, units=doc.units, items=items,
                views=[asdict(v) for v in sheet.views],
                omissions=[_plain(o) for o in sheet.omissions], sources=[asdict(s) for s in sheet.sources],
                svg=svg, export_report=svg_report(doc,sheet,svg.encode('utf-8')))
            encoded = sheet_bytes(_pack_item_records(_pack_view_references(_pack_styles(payload))), min(MAX_SHEET_BYTES, limit - total))
            total += len(encoded)
            digest = hashlib.sha256(encoded).hexdigest()
            resource = f'drawing-sheet-{digest}.json'
            (directory / resource).write_bytes(encoded)
            descriptor.update(resource=resource, bytes=len(encoded), sha256=digest)
            needed.update(i.geometry['reference'] for i in sheet.items if i.geometry['kind'] == 'image')
        sheets.append(descriptor)
    images = []
    for image in doc.images:
        if image.reference not in needed:
            continue
        resource = None
        if image.data is not None:
            if len(image.data) > limit - total:
                raise ValueError('Drawing resource byte limit exceeded')
            total += len(image.data)
            extension = {'image/png': 'png', 'image/jpeg': 'jpg'}[image.mime_type]
            resource = f'drawing-image-{image.reference}.{extension}'
            (directory / resource).write_bytes(image.data)
        images.append(dict(reference=image.reference, resource=resource, status=image.status,
            width=image.width, height=image.height, sha256=image.sha256,
            source=asdict(image.source), diagnostic=image.diagnostic))
    status = 'experimental_partial' if any(s['resource'] for s in sheets) else 'unavailable'
    scene['drawing'] = dict(api_version=1, source_sha256=doc.source_sha256, status=status, sheet_status=doc.sheet_status,
        units=doc.units, length_unit=doc.length_unit, millimeters_per_unit=doc.millimeters_per_unit,
        qualified=False, complete=False, current_state='unverified', snapshot_kind='saved',
        reference_freshness='unverified', experimental=True,
        allow_partial=options.allow_partial, sheets=sheets, images=images)
    scene['stages'].update(geometry=status, conversion='not_applicable', tessellation='not_applicable')
    return scene
