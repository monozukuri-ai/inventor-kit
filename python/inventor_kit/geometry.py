"""Stored kernel candidates, explicit selection and unresolved state bindings."""
from __future__ import annotations

from dataclasses import dataclass
from .document import Diagnostic, FormatProfile, SourceSpan


@dataclass(frozen=True)
class GeometrySelection:
    policy: str
    requested_id: str | None
    selected_id: str | None
    status: str
    basis: str | None
    state_verification: str
    reason: str | None


@dataclass(frozen=True)
class SegmentInventory:
    token: str
    meta_source: SourceSpan | None
    bulk_source: SourceSpan | None
    segment_id: str | None
    name: str | None
    registry_indices: tuple[int, ...]
    status: str
    meta_compressed_source: SourceSpan | None
    meta_codec: str | None
    meta_expanded_bytes: int | None
    meta_state_words: tuple[int, ...] | None
    record_count: int | None


@dataclass(frozen=True)
class CarrierHeader:
    state: int
    kind: int
    value: int
    schema: int
    source: SourceSpan
    interpretation: str


@dataclass(frozen=True)
class CandidateCarrier:
    stream: str
    source_id: str
    record_ordinal: int
    segment_major: int
    kernel_offset: int
    kernel_end: int
    codec: str
    expanded_bytes: int
    meta_expanded_bytes: int
    selected_key: int
    enabled: bool
    delta_state: int
    history_reference: int
    save_version: int | None
    integer_width: int | None
    header_scale: float | None
    has_history: bool | None


@dataclass(frozen=True)
class KernelCandidate:
    id: str
    segment_index: int
    registry_index: int
    segment_id: str
    record_ordinal: int
    record_type: str
    record_source: SourceSpan
    compressed_source: SourceSpan
    kernel_source: SourceSpan | None
    kernel_relative_source: SourceSpan | None
    kernel_sha256: str | None
    header: CarrierHeader | None
    footer_source: SourceSpan | None
    carrier: CandidateCarrier | None
    table_status: str
    profile_status: str
    solved_records: int | None
    solved_source: SourceSpan | None
    history_source: SourceSpan | None
    database_binding: str
    state_binding: str
    suppression: str
    history_replay: str
    diagnostics: tuple[Diagnostic, ...]


@dataclass(frozen=True)
class GeometryInventory:
    api_version: int
    source_sha256: str | None
    status: str
    segments: tuple[SegmentInventory, ...]
    candidates: tuple[KernelCandidate, ...]
    selection: GeometrySelection
    diagnostics: tuple[Diagnostic, ...]


def _span(value):
    return SourceSpan(**value) if value is not None else None


def _diagnostic(value):
    return Diagnostic(**{**value, 'source': _span(value['source']),
                         'profile': FormatProfile(**value['profile']) if value['profile'] else None})


def _geometry(data) -> GeometryInventory:
    if data['api_version'] != 1:
        raise ValueError('Unsupported Rust geometry inventory API version')
    segments = []
    for raw in data['segments']:
        fields = dict(raw)
        for key in ['meta_source', 'bulk_source', 'meta_compressed_source']:
            fields[key] = _span(raw[key])
        fields['registry_indices'] = tuple(raw['registry_indices'])
        fields['meta_state_words'] = tuple(raw['meta_state_words']) if raw['meta_state_words'] is not None else None
        segments.append(SegmentInventory(**fields))
    candidates = []
    for raw in data['candidates']:
        fields = dict(raw)
        for key in ['record_source', 'compressed_source', 'kernel_source', 'kernel_relative_source',
                    'footer_source', 'solved_source', 'history_source']:
            fields[key] = _span(raw[key])
        fields['header'] = CarrierHeader(**{**raw['header'], 'source': _span(raw['header']['source'])}) if raw['header'] else None
        fields['carrier'] = CandidateCarrier(**raw['carrier']) if raw['carrier'] else None
        fields['diagnostics'] = tuple(_diagnostic(d) for d in raw['diagnostics'])
        candidates.append(KernelCandidate(**fields))
    return GeometryInventory(api_version=data['api_version'], source_sha256=data['source_sha256'], status=data['status'],
                             segments=tuple(segments), candidates=tuple(candidates), selection=GeometrySelection(**data['selection']),
                             diagnostics=tuple(_diagnostic(d) for d in data['diagnostics']))
