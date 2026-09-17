"""Experimental saved IDW display, without Inventor or Python 3D imports.

Coordinates remain in source units. A stored sheet binding is not a claim of
physical units, complete annotation coverage, or current drawing state.
"""
from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
from types import MappingProxyType
from typing import Mapping

from . import _inventor
from .document import DocumentInfo, SourceSpan, _document
from .limits import Limits, encoded


def _freeze(value):
    if isinstance(value, dict):
        return MappingProxyType({k: _freeze(v) for k, v in value.items()})
    if isinstance(value, list):
        return tuple(_freeze(v) for v in value)
    return value


def _plain(value):
    if isinstance(value, Mapping):
        return {k: _plain(v) for k, v in value.items()}
    if isinstance(value, tuple):
        return [_plain(v) for v in value]
    return value


@dataclass(frozen=True)
class DrawingItem:
    id: str
    geometry: Mapping
    style: Mapping
    source: SourceSpan
    segment_id: str
    record_ordinal: int
    placement_record: int
    group_path: tuple[int, ...]
    transform: tuple[tuple[float, ...], ...]


@dataclass(frozen=True)
class DrawingImage:
    reference: int
    status: str
    mime_type: str | None
    width: int | None
    height: int | None
    sha256: str | None
    data: bytes | None
    source: SourceSpan
    diagnostic: str | None


@dataclass(frozen=True)
class DrawingSheet:
    id: str
    index: int
    name: str
    status: str
    size_in_source_units: tuple[float, float] | None
    space_segment: str | None
    items: tuple[DrawingItem, ...]
    omissions: tuple[Mapping, ...]
    sources: tuple[SourceSpan, ...]
    diagnostics: tuple[str, ...]


@dataclass(frozen=True)
class DrawingDocument:
    api_version: int
    source_sha256: str
    status: str
    sheet_status: str
    units: str
    length_unit: str | None
    millimeters_per_unit: float | None
    qualified: bool
    complete: bool
    current_state: str
    metadata: DocumentInfo
    sheets: tuple[DrawingSheet, ...]
    images: tuple[DrawingImage, ...]
    diagnostics: tuple[Mapping, ...]

    def sheet(self, sheet_id: str) -> DrawingSheet:
        """Select by source-local ID; names may be duplicated or localized."""
        for sheet in self.sheets:
            if sheet.id == sheet_id:
                return sheet
        raise KeyError(sheet_id)


def _decode(raw):
    if raw['api_version'] != 1:
        raise ValueError('Unsupported drawing API version')
    preview, index = raw['preview'], raw['sheets']
    spaces = {s['segment_id']: s for s in preview['spaces']}
    diagnostics = list(raw['diagnostics'])
    def diagnostic(message):
        diagnostics.append(dict(code='drawing.unverified', severity='warning', message=message, source=None))
    for message in (*index['diagnostics'], *preview['diagnostics']):
        diagnostic(message)
    sheets, used = [], set()
    for sheet in index['sheets']:
        space = spaces.get(sheet['space_segment'])
        items = []
        if space is not None:
            used.add(sheet['space_segment'])
            for i in space['items']:
                item_id = f"{sheet['id']}/{i['segment_id']}/{i['placement_record']}/{i['record_ordinal']}"
                items.append(DrawingItem(item_id, _freeze(i['geometry']), _freeze(i['style']),
                    SourceSpan(**i['source']), i['segment_id'], i['record_ordinal'],
                    i['placement_record'], tuple(i['group_path']), tuple(map(tuple, i['transform']))))
        issues = list(sheet['diagnostics'])
        if space is None:
            issues.append('stored_display_unavailable')
        sheets.append(DrawingSheet(sheet['id'], sheet['index'], sheet['name'],
            'experimental_partial' if space is not None else 'unavailable',
            tuple(sheet['size_in_source_units']) if sheet['size_in_source_units'] is not None else None,
            sheet['space_segment'], tuple(items),
            tuple(_freeze(o) for o in space['omitted']) if space else (),
            tuple(SourceSpan(**s) for s in sheet['sources']), tuple(issues)))
    for segment in spaces.keys() - used:
        diagnostic(f'display_space_not_in_document_sheet_list: {segment}')
    needed = {i.geometry['reference'] for s in sheets for i in s.items if i.geometry['kind'] == 'image'}
    images = tuple(DrawingImage(i['reference'], i['status'], i['mime_type'], i['width'], i['height'],
        i['sha256'], bytes.fromhex(i['data']) if i['data'] is not None else None,
        SourceSpan(**i['source']), i['diagnostic']) for i in raw['images'] if i['reference'] in needed)
    return DrawingDocument(1, raw['source_sha256'],
        'experimental_partial' if any(s.status == 'experimental_partial' for s in sheets) else 'unavailable',
        index['status'], preview['units'], index['length_unit'], index['millimeters_per_unit'],
        False, False, 'unverified', _document(raw['metadata']), tuple(sheets), images,
        tuple(_freeze(d) for d in diagnostics))


def read_drawing(data: bytes, *, source_id: str = 'inventor-input', limits: Limits | None = None) -> DrawingDocument:
    """Read experimental IDW sheets/display from one immutable input snapshot.

    Unsupported profiles return unavailable with metadata/diagnostics. Invalid
    containers and non-IDW roots raise ValueError. External references are never
    opened. Each native stage independently applies the supplied resource limits.
    """
    return _decode(json.loads(_inventor.read_drawing(data, source_id, encoded(limits))))


def read_drawing_file(path: str | Path, *, limits: Limits | None = None) -> DrawingDocument:
    from . import _file_bytes
    path = Path(path)
    return read_drawing(_file_bytes(path, limits), source_id=str(path), limits=limits)
