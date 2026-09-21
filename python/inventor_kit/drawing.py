"""Experimental saved IDW display, without Inventor or Python 3D imports.

Coordinates remain in source units. A stored sheet binding is not a claim of
physical units, complete annotation coverage, or current drawing state.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, fields
import json
import math
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
class DrawingLimits:
    """Tighten native drawing ceilings; zero denies the resource.

    Counts apply to expanded display items, polyline points and UTF-8 text
    (including copied font-family names). Reference work is bounded per native
    stage. These limits do not bound process RSS or wall-clock time.
    """
    max_sheets: int = 256
    max_views: int = 4096
    max_display_items: int = 100_000
    max_polyline_points: int = 1_000_000
    max_text_bytes: int = 16 * 1024 * 1024
    max_reference_visits: int = 500_000
    max_nesting_depth: int = 128
    max_image_bytes: int = 16 * 1024 * 1024
    max_image_pixels: int = 16_777_216
    max_output_bytes: int = 64 * 1024 * 1024

    def __post_init__(self):
        for field in fields(self):
            value = getattr(self, field.name)
            if type(value) is not int or not 0 <= value <= field.default:
                raise ValueError(f'{field.name} must be an integer from 0 to {field.default}')


def _drawing_limits_json(value):
    if value is None:
        value = DrawingLimits()
    if not isinstance(value, DrawingLimits):
        raise TypeError('drawing_limits must be DrawingLimits or None')
    value.__post_init__()
    return json.dumps(asdict(value))


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
class DrawingView:
    """Reachable saved view and cache, in source units.

    The placement matrix positions saved pixels. It does not identify a model
    projection, parent view, clipping region or rotation already baked into the
    cache. None means unresolved, never zero rotation or a base view.
    """
    id: str
    name: str
    placement_record: int
    placement_transform: tuple[tuple[float, ...], ...]
    cache_bounds: tuple[float, ...] | None
    image_reference: int | None
    item_ids: tuple[str, ...]
    source: SourceSpan
    diagnostics: tuple[str, ...]
    view_type: str | None = None
    parent_view_id: str | None = None
    rotation: float | None = None


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
    views: tuple[DrawingView, ...] = ()

    @property
    def content_coverage(self) -> str:
        # Unknown record roles cannot be counted as absent or complete content.
        return 'unknown'


@dataclass(frozen=True)
class SheetDisplay:
    """A paper-space display in millimeters, admitted by render_sheet."""
    source_sha256: str
    sheet_id: str
    size_mm: tuple[float, float]
    items: tuple[DrawingItem, ...]
    content_coverage: str
    appearance: str
    display_status: str
    omissions: tuple[Mapping, ...]
    diagnostics: tuple[Mapping, ...]
    snapshot_kind: str = 'saved'
    reference_freshness: str = 'unverified'


class DrawingDisplayError(ValueError):
    """A refused display request, retaining its sheet and structured reasons."""
    def __init__(self, sheet_id, diagnostics, omissions=()):
        self.sheet_id = sheet_id
        self.diagnostics = tuple(_freeze(d) for d in diagnostics)
        self.omissions = tuple(omissions)
        super().__init__('; '.join(d['message'] for d in self.diagnostics))


def _millimeter_item(item, scale):
    from dataclasses import replace
    geometry, style = _plain(item.geometry), _plain(item.style)
    kind = geometry['kind']
    def point(value):
        result = [v * scale for v in value]
        if not all(math.isfinite(v) for v in result):
            raise ValueError('Drawing millimeter coordinate overflow')
        return result
    if kind == 'polyline':
        geometry['points'] = [point(p) for p in geometry['points']]
    elif kind in ('curve', 'image'):
        for key in (('center', 'u', 'v') if kind == 'curve' else ('origin', 'u', 'v')):
            geometry[key] = point(geometry[key])
    elif kind == 'text':
        geometry['position'] = point(geometry['position'])
        if geometry['font'] is not None:
            geometry['font']['height_candidate'] = point([geometry['font']['height_candidate']])[0]
    else:
        raise ValueError('Unsupported drawing geometry')
    if style.get('width') is not None:
        style['width'] = point([style['width']])[0]
    if style.get('dash') is not None:
        style['dash'] = point(style['dash'])
    # The original placement remains provenance; do not apply it twice.
    return replace(item, geometry=_freeze(geometry), style=_freeze(style))


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
        """Select by input-bound ID; names may be duplicated or localized."""
        for sheet in self.sheets:
            if sheet.id == sheet_id:
                return sheet
        raise KeyError(sheet_id)

    @property
    def snapshot_kind(self) -> str:
        return 'saved'

    @property
    def reference_freshness(self) -> str:
        return 'unverified'

    def render_sheet(self, *, sheet_id: str, allow_partial: bool = False) -> SheetDisplay:
        """Request an admitted millimeter display, never guess physical units.

        Current experimental profiles have unverified units and are refused even
        with allow_partial=True. sheet().items remains the explicit experimental
        source-unit inspection path. This method never starts a browser.
        """
        if type(allow_partial) is not bool:
            raise TypeError('allow_partial must be a bool')
        try:
            sheet = self.sheet(sheet_id)
        except KeyError:
            raise DrawingDisplayError(sheet_id, [dict(code='drawing.invalid_sheet_id',
                message='The sheet ID does not belong to this input.')]) from None
        reasons = []
        def reject(code, message):
            reasons.append(dict(code=code, message=message, sheet_id=sheet.id))
        scale = self.millimeters_per_unit
        if self.length_unit is None or scale is None or not math.isfinite(scale) or scale <= 0:
            reject('drawing.units_unverified', 'Physical drawing units are unverified; millimeter display is unavailable.')
        if sheet.status == 'unavailable' or sheet.size_in_source_units is None:
            reject('drawing.sheet_unavailable', 'Stored sheet ownership or paper placement is unavailable.')
        if sheet.content_coverage != 'complete' and not allow_partial:
            reject('drawing.coverage_unverified', 'Complete saved content is not verified; a partial display must be explicitly requested.')
        if reasons:
            raise DrawingDisplayError(sheet.id, reasons, sheet.omissions)
        size = tuple(v * scale for v in sheet.size_in_source_units)
        if not all(math.isfinite(v) and v > 0 for v in size):
            raise DrawingDisplayError(sheet.id, [dict(code='drawing.invalid_size', message='Invalid millimeter sheet size.')], sheet.omissions)
        return SheetDisplay(self.source_sha256, sheet.id, size,
            tuple(_millimeter_item(i, scale) for i in sheet.items), sheet.content_coverage,
            'unverified', 'partial', sheet.omissions, self.diagnostics)


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
        sheet_id = f"{raw['source_sha256']}/{sheet['id']}"
        space = spaces.get(sheet['space_segment'])
        items, placement_items = [], {}
        if space is not None:
            used.add(sheet['space_segment'])
            for i in space['items']:
                item_id = f"{sheet_id}/{i['segment_id']}/{i['placement_record']}/{i['record_ordinal']}"
                items.append(DrawingItem(item_id, _freeze(i['geometry']), _freeze(i['style']),
                    SourceSpan(**i['source']), i['segment_id'], i['record_ordinal'],
                    i['placement_record'], tuple(i['group_path']), tuple(map(tuple, i['transform']))))
                placement_items.setdefault(i['placement_record'], []).append(item_id)
        issues = list(sheet['diagnostics'])
        views = tuple(DrawingView(
            f"{sheet_id}/view-{v['placement_record']}", v['name'], v['placement_record'],
            tuple(map(tuple, v['placement_transform'])),
            tuple(v['cache_bounds']) if v['cache_bounds'] is not None else None, v['image_reference'],
            tuple(placement_items.get(v['placement_record'], ())),
            SourceSpan(**v['source']), tuple(v['diagnostics'])) for v in space['views']) if space else ()
        if space is None:
            issues.append('stored_display_unavailable')
        sheets.append(DrawingSheet(sheet_id, sheet['index'], sheet['name'],
            'experimental_partial' if space is not None else 'unavailable',
            tuple(sheet['size_in_source_units']) if sheet['size_in_source_units'] is not None else None,
            sheet['space_segment'], tuple(items),
            tuple(_freeze(o) for o in space['omitted']) if space else (),
            tuple(SourceSpan(**s) for s in sheet['sources']), tuple(issues), views))
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


def read_drawing(data: bytes, *, source_id: str = 'inventor-input', limits: Limits | None = None,
                 drawing_limits: DrawingLimits | None = None) -> DrawingDocument:
    """Read experimental IDW sheets/display from one immutable input snapshot.

    Unsupported profiles return unavailable with metadata/diagnostics. Invalid
    containers and non-IDW roots raise ValueError. External references are never
    opened. Each native stage independently applies the supplied resource limits.
    """
    return _decode(json.loads(_inventor.read_drawing(data, source_id, encoded(limits), _drawing_limits_json(drawing_limits))))


def read_drawing_file(path: str | Path, *, limits: Limits | None = None,
                 drawing_limits: DrawingLimits | None = None) -> DrawingDocument:
    from . import _file_bytes
    _drawing_limits_json(drawing_limits)  # Reject invalid options before opening a file.
    path = Path(path)
    return read_drawing(_file_bytes(path, limits), source_id=str(path), limits=limits, drawing_limits=drawing_limits)
