"""Read Inventor parts through the Rust CFB/RSe/SAB pipeline."""
from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
from typing import TYPE_CHECKING

from . import _inventor
from .limits import Limits, effective as _effective_limits, encoded as _encoded_limits
from .document import (DocumentInfo, Property, PropertySet, Diagnostic, SourceSpan,
                       Thumbnail, FileTime, OleDate, ClipboardData, PropertyArray, _document)
from .geometry import GeometryInventory, GeometrySelection, KernelCandidate, SegmentInventory, _geometry
from .assembly import (AssemblyDocument, SavedOccurrence, AssemblyDefinition, AssemblyInstance,
                       InventorAssembly, FileSystemResolver, AssemblyConversion, AssemblyConversionError,
                       AssemblyOmission, inspect_assembly, inspect_assembly_file, read_assembly_file)

if TYPE_CHECKING:
    from cq_acis import AcisModel


def capabilities() -> dict:
    """Return declared bounded support; does not claim host/CI or vendor validation."""
    from importlib.resources import files
    return json.loads(files(__package__).joinpath('capabilities.json').read_text(encoding='utf-8'))


@dataclass(frozen=True)
class InventorDocument:
    """Inventory and the sole typed part carrier, when structurally resolved.

    ``decoded_subset`` describes the shared model, not successful exact geometry
    conversion. Inspect model diagnostics and call to_cadquery for that check.
    """
    summary: dict
    model: AcisModel | None
    kernel_bytes: bytes | None
    metadata: DocumentInfo | None = None
    geometry: GeometryInventory | None = None

    def to_cadquery(self):
        if self.model is None:
            if self.metadata is not None and self.metadata.stages.geometry == "not_attempted":
                raise ValueError("Geometry was not requested; use read or read_file")
            raise ValueError("Inventor geometry unavailable: " + "; ".join(self.summary["diagnostics"]))
        if not self.model.bodies():
            raise ValueError("The stored B-rep table contains no decoded bodies")
        from cq_acis import to_cadquery
        return to_cadquery(self.model)


def read(data: bytes, *, source_id: str = "inventor-input", candidate_id: str | None = None,
         require_current_state: bool = False, limits: Limits | None = None) -> InventorDocument:
    """Decode bytes offline. Invalid containers raise; unsupported profiles report."""
    summary, model, kernel = _inventor.read(data, source_id, candidate_id, require_current_state, _encoded_limits(limits))
    summary = json.loads(summary)
    return InventorDocument(summary, model, kernel, _document(summary["document"]), _geometry(summary["geometry"]))


def inspect(data: bytes, *, source_id: str = "inventor-input", include_candidates: bool = False,
            limits: Limits | None = None) -> InventorDocument:
    """Read metadata; optionally inventory SAB candidates without Python geometry imports."""
    summary = json.loads(_inventor.inspect(data, source_id, include_candidates, _encoded_limits(limits)))
    return InventorDocument(summary, None, None, _document(summary["document"]), _geometry(summary["geometry"]))


def _file_bytes(path: Path, limits: Limits | None = None) -> bytes:
    limit = _effective_limits(limits).max_file_bytes
    if path.stat().st_size > limit:
        raise ValueError("file byte limit exceeded")
    with path.open("rb") as source:
        data = source.read(limit + 1)
    if len(data) > limit:
        raise ValueError("file byte limit exceeded")
    return data


def read_file(path: str | Path, *, candidate_id: str | None = None, require_current_state: bool = False,
              limits: Limits | None = None) -> InventorDocument:
    path = Path(path)
    return read(_file_bytes(path, limits), source_id=str(path), candidate_id=candidate_id,
                require_current_state=require_current_state, limits=limits)


def inspect_file(path: str | Path, *, include_candidates: bool = False, limits: Limits | None = None) -> InventorDocument:
    path = Path(path)
    return inspect(_file_bytes(path, limits), source_id=str(path), include_candidates=include_candidates, limits=limits)


__all__ = ["Limits", "capabilities", "InventorDocument", "DocumentInfo", "Property", "PropertySet", "Diagnostic", "SourceSpan",
           "Thumbnail", "FileTime", "OleDate", "ClipboardData", "PropertyArray", "GeometryInventory", "GeometrySelection",
           "KernelCandidate", "SegmentInventory", "read", "read_file", "inspect", "inspect_file",
           "AssemblyDocument", "SavedOccurrence", "AssemblyDefinition", "AssemblyInstance", "InventorAssembly",
           "FileSystemResolver", "AssemblyConversion", "AssemblyConversionError", "AssemblyOmission",
           "inspect_assembly", "inspect_assembly_file", "read_assembly_file"]
