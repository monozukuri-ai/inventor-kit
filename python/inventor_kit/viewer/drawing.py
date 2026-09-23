"""Publish input-bound sheets only after the complete worker succeeds."""
from dataclasses import asdict
import hashlib
import json
import re

from ..drawing import read_drawing, _plain

MAX_SHEET_BYTES = 32 * 1024 * 1024
MAX_DRAWING_BYTES = 128 * 1024 * 1024


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
        payload = json.loads(body)
        if (payload['schema_version'] != 1 or payload['scene_kind'] != 'drawing_sheet'
                or payload['source_sha256'] != source or payload['sheet_id'] != sheet['id']
                or payload['units'] != drawing['units'] or len(payload['items']) != sheet['item_count']
                or len(payload['omissions']) != sheet['omission_count']):
            raise ValueError('Drawing sheet payload identity/count mismatch')
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
            items = [dict(id=i.id, geometry=_plain(i.geometry), style=_plain(i.style),
                source=asdict(i.source), segment_id=i.segment_id, record_ordinal=i.record_ordinal,
                placement_record=i.placement_record, group_path=list(i.group_path)) for i in sheet.items]
            payload = dict(schema_version=1, scene_kind='drawing_sheet', source_sha256=doc.source_sha256,
                sheet_id=sheet.id, units=doc.units, items=items,
                views=[asdict(v) for v in sheet.views],
                omissions=[_plain(o) for o in sheet.omissions], sources=[asdict(s) for s in sheet.sources])
            encoded = sheet_bytes(payload, min(MAX_SHEET_BYTES, limit - total))
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
