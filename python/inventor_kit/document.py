"""Typed document observations. Stored properties never imply all-state values."""
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from typing import Any


@dataclass(frozen=True)
class SourceSpan:
    source_id: str
    stream: str
    byte_domain: str
    start_offset: int
    end_offset: int


@dataclass(frozen=True)
class FormatProfile:
    rse_db_schema: int | None
    segment_major: int | None
    sab_version: int | None


@dataclass(frozen=True)
class Diagnostic:
    code: str
    severity: str
    message: str
    source: SourceSpan | None
    record: int | None
    fmtid: str | None
    pid: int | None
    profile: FormatProfile | None


@dataclass(frozen=True)
class FileTime:
    """100 ns ticks since 1601; retain precision even when datetime cannot."""
    ticks_100ns: int

    @property
    def datetime_utc(self) -> datetime | None:
        try:
            return datetime(1601, 1, 1, tzinfo=timezone.utc) + timedelta(microseconds=self.ticks_100ns // 10)
        except OverflowError:
            return None


@dataclass(frozen=True)
class OleDate:
    """Original OLE DATE days; interpretation and timezone are not guessed."""
    days: float


@dataclass(frozen=True)
class ClipboardData:
    format: int
    data: bytes


@dataclass(frozen=True)
class ArrayDimension:
    size: int
    lower_bound: int


@dataclass(frozen=True)
class PropertyArray:
    """Values in serialized order; empty dimensions denotes a vector."""
    element_type: int
    dimensions: tuple[ArrayDimension, ...]
    values: tuple[Any, ...]


@dataclass(frozen=True)
class DictionaryEntry:
    pid: int
    name: str


@dataclass(frozen=True)
class Property:
    fmtid: str
    pid: int
    name: str | None
    semantic_name: str | None
    type_code: int | None
    status: str
    value_kind: str | None
    value: Any
    raw_data: bytes
    source: SourceSpan
    storage_scope: str
    state_binding: str


@dataclass(frozen=True)
class PropertySet:
    fmtid: str
    stream: str
    version: int
    system_identifier: int
    clsid: str
    code_page: int | None
    storage_scope: str
    state_binding: str
    status: str
    source: SourceSpan
    properties: tuple[Property, ...]


@dataclass(frozen=True)
class Identification:
    kind: str
    status: str
    basis: str
    root_clsid: str


@dataclass(frozen=True)
class DatabaseInfo:
    stream: str
    schema: int | None
    status: str
    name: str | None
    source: SourceSpan


@dataclass(frozen=True)
class SegmentInfo:
    name: str
    kind: str
    id: str
    major: int
    source: SourceSpan


@dataclass(frozen=True)
class UnparsedPropertyStream:
    source: SourceSpan
    status: str
    raw_data: bytes | None


@dataclass(frozen=True)
class Thumbnail:
    mime_type: str
    data: bytes
    width: int
    height: int
    source: SourceSpan
    fmtid: str
    pid: int
    profile: str
    validation: str
    state_binding: str


@dataclass(frozen=True)
class DocumentStages:
    identification: str
    properties: str
    registry: str
    geometry: str
    conversion: str
    state: str
    references: str


@dataclass(frozen=True)
class DocumentInfo:
    api_version: int
    identification: Identification
    databases: tuple[DatabaseInfo, ...]
    segments: tuple[SegmentInfo, ...]
    property_sets: tuple[PropertySet, ...]
    unparsed_property_streams: tuple[UnparsedPropertyStream, ...]
    thumbnails: tuple[Thumbnail, ...]
    diagnostics: tuple[Diagnostic, ...]
    stages: DocumentStages
    geometry_profile: FormatProfile

    def find_properties(self, *, fmtid: str | None = None, pid: int | None = None,
                        semantic_name: str | None = None) -> tuple[Property, ...]:
        """Return all matches; do not select a state/owner or collapse duplicates."""
        normalized = fmtid.strip('{}').lower() if fmtid is not None else None
        return tuple(p for s in self.property_sets for p in s.properties
                     if (normalized is None or p.fmtid == normalized)
                     and (pid is None or p.pid == pid)
                     and (semantic_name is None or p.semantic_name == semantic_name))


def _decimal(integer, scale, negative):
    digits = str(integer).zfill(scale + 1)
    text = digits if scale == 0 else digits[:-scale] + '.' + digits[-scale:]
    return Decimal(('-' if negative else '') + text)


def _value(data):
    if data is None:
        return None
    kind = data['kind']
    if kind in ('empty', 'null'):
        return None
    if kind in ('signed', 'unsigned', 'float', 'bool', 'text', 'guid'):
        return data['value']
    if kind == 'filetime':
        return FileTime(data['ticks_100ns'])
    if kind == 'ole_date':
        return OleDate(data['days'])
    if kind == 'currency':
        return _decimal(abs(data['scaled_value']), 4, data['scaled_value'] < 0)
    if kind == 'decimal':
        # Construct from a decimal string to avoid ambient Decimal precision loss.
        integer = (data['high'] << 64) | data['low']
        return _decimal(integer, data['scale'], data['negative'])
    if kind == 'blob':
        return bytes.fromhex(data['data'])
    if kind == 'clipboard':
        return ClipboardData(data['format'], bytes.fromhex(data['data']))
    if kind == 'sequence':
        return PropertyArray(data['element_type'], tuple(ArrayDimension(**d) for d in data['dimensions']),
                             tuple(_value(v) for v in data['values']))
    if kind == 'dictionary':
        return tuple(DictionaryEntry(**d) for d in data['entries'])
    raise ValueError(f'Unsupported document value kind: {kind}')


def _document(data) -> DocumentInfo:
    if data['api_version'] != 1:
        raise ValueError('Unsupported Rust document API version')
    sets = []
    for s in data['property_sets']:
        properties = []
        for p in s['properties']:
            properties.append(Property(**{**p, 'fmtid': s['fmtid'], 'source': SourceSpan(**p['source']),
                                          'value_kind': p['value']['kind'] if p['value'] is not None else None,
                                          'value': _value(p['value']), 'raw_data': bytes.fromhex(p['raw_data']),
                                          'storage_scope': s['storage_scope'], 'state_binding': s['state_binding']}))
        sets.append(PropertySet(**{**s, 'source': SourceSpan(**s['source']), 'properties': tuple(properties)}))
    def located(cls, d):
        return cls(**{**d, 'source': SourceSpan(**d['source'])})
    return DocumentInfo(
        api_version=data['api_version'], identification=Identification(**data['identification']),
        databases=tuple(located(DatabaseInfo, d) for d in data['databases']),
        segments=tuple(located(SegmentInfo, d) for d in data['segments']), property_sets=tuple(sets),
        unparsed_property_streams=tuple(located(UnparsedPropertyStream, {**d, 'raw_data': bytes.fromhex(d['raw_data']) if d['raw_data'] is not None else None}) for d in data['unparsed_property_streams']),
        thumbnails=tuple(located(Thumbnail, {**d, 'data': bytes.fromhex(d['data'])}) for d in data['thumbnails']),
        diagnostics=tuple(Diagnostic(**{**d, 'source': SourceSpan(**d['source']) if d['source'] else None,
                                        'profile': FormatProfile(**d['profile']) if d['profile'] else None}) for d in data['diagnostics']),
        stages=DocumentStages(**data['stages']), geometry_profile=FormatProfile(**data['geometry_profile']))
