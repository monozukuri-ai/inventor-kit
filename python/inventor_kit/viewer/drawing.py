"""Publish bounded experimental 2D resources; no CAD conversion imports."""
from dataclasses import asdict

from ..drawing import read_drawing, _plain
from .scene import diagnostic


def build_drawing_scene(data, directory, options, scene):
    scene['units'] = 'source_units_unverified'
    if (options.candidate_id or options.require_current_state or options.body_ids or
            options.search_roots or options.allow_unverified_state or options.allow_partial):
        raise ValueError('IPT/IAM selection options do not apply to IDW drawings')
    if not options.experimental_drawing:
        scene['stages']['geometry'] = 'not_enabled'
        diagnostic(scene, 'drawing.experimental_opt_in',
                   'Use --experimental-drawing to display supported saved 2D elements. Drawing completeness, units and current state are unverified.', 'warning')
        return scene
    doc = read_drawing(data, source_id=scene['source']['name'], limits=options.limits)
    scene['diagnostics'].extend(_plain(d) for d in doc.diagnostics)
    sheets = []
    for sheet in doc.sheets:
        items = []
        for item in sheet.items:
            items.append(dict(id=item.id, geometry=_plain(item.geometry), style=_plain(item.style),
                source=asdict(item.source), segment_id=item.segment_id, record_ordinal=item.record_ordinal,
                placement_record=item.placement_record, group_path=list(item.group_path)))
        sheets.append(dict(id=sheet.id, index=sheet.index, name=sheet.name, status=sheet.status,
            size_in_source_units=list(sheet.size_in_source_units) if sheet.size_in_source_units is not None else None, items=items,
            omissions=[_plain(o) for o in sheet.omissions], sources=[asdict(s) for s in sheet.sources],
            diagnostics=list(sheet.diagnostics)))
    images, total = [], 0
    for image in doc.images:
        resource = None
        if image.data is not None:
            total += len(image.data)
            if total > options.max_buffer_bytes:
                raise ValueError('Drawing image buffer limit exceeded')
            extension = {'image/png': 'png', 'image/jpeg': 'jpg'}[image.mime_type]
            resource = f'drawing-image-{image.reference}.{extension}'
            (directory / resource).write_bytes(image.data)
        images.append(dict(reference=image.reference, resource=resource, status=image.status,
            width=image.width, height=image.height, sha256=image.sha256,
            source=asdict(image.source), diagnostic=image.diagnostic))
    scene['drawing'] = dict(api_version=1, status=doc.status, sheet_status=doc.sheet_status,
        units=doc.units, length_unit=doc.length_unit, millimeters_per_unit=doc.millimeters_per_unit,
        qualified=False, complete=False, current_state='unverified', sheets=sheets, images=images)
    scene['stages'].update(geometry=doc.status, conversion='not_applicable', tessellation='not_applicable')
    return scene
