"""Saved IAM references and placements; filesystem work and graph ownership are Rust's.

The native profile is experimental. Current Model State, visibility and substitute
selection are not verified. Geometry conversion requires explicit acknowledgement.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
from typing import Any

from . import _inventor
from .limits import Limits, encoded as _encoded_limits

Matrix = tuple[tuple[float, ...], ...]


def _matrix(value) -> Matrix | None:
    return None if value is None else tuple(tuple(row) for row in value)


@dataclass(frozen=True)
class SavedOccurrence:
    occurrence_id: int
    reference_id: int
    name: str | None
    suppressed: bool | None
    visible: bool | None
    substitute: bool | None
    status: str
    local_transform_mm: Matrix | None
    source: dict
    identity: dict | None
    placement: dict | None


@dataclass(frozen=True)
class AssemblyDocument:
    """One file's inventory, including raw state words and byte provenance."""
    summary: dict
    occurrences: tuple[SavedOccurrence, ...]

    @property
    def references(self) -> tuple[dict, ...]:
        return tuple(self.summary["ufrx"]["references"])

    @property
    def stored_occurrences(self) -> tuple[dict, ...]:
        """UFRx headers/properties with tags and spans; their semantics are unknown."""
        return tuple(self.summary["ufrx"]["occurrences"])

    @property
    def diagnostics(self) -> tuple[dict, ...]:
        return tuple(self.summary["diagnostics"])

    @property
    def status(self) -> str:
        return self.summary["status"]


def _document(summary: dict) -> AssemblyDocument:
    return AssemblyDocument(summary, tuple(
        SavedOccurrence(**{**o, "local_transform_mm": _matrix(o["local_transform_mm"])})
        for o in summary["occurrences"]))


def inspect_assembly(data: bytes, *, source_id: str = "inventor-input", limits: Limits | None = None) -> AssemblyDocument:
    """Inspect saved references without opening other files or importing CadQuery."""
    return _document(json.loads(_inventor.inspect_assembly(data, source_id, _encoded_limits(limits))))


def inspect_assembly_file(path: str | Path, *, limits: Limits | None = None) -> AssemblyDocument:
    from . import _file_bytes
    path = Path(path)
    return inspect_assembly(_file_bytes(path, limits), source_id=str(path), limits=limits)


@dataclass(frozen=True)
class AssemblyDefinition:
    key: str
    path: str
    document: AssemblyDocument
    references: tuple[dict, ...]
    source_paths: tuple[str, ...]


@dataclass(frozen=True)
class AssemblyInstance:
    parent: int | None
    owner_definition: int
    occurrence_index: int
    occurrence_id: int
    path: tuple[int, ...]
    name: str | None
    definition: int | None
    resolution: str
    suppressed: bool | None
    visible: bool | None
    substitute: bool | None
    local_transform_mm: Matrix | None
    world_transform_mm: Matrix | None


@dataclass(frozen=True)
class AssemblyOmission:
    instance: int
    path: tuple[int, ...]
    reason: str
    detail: str


@dataclass(frozen=True)
class AssemblyConversion:
    assembly: Any
    omissions: tuple[AssemblyOmission, ...]
    converted_instances: int
    converted_definitions: int
    reference_issues: tuple[dict, ...] = ()
    diagnostics: tuple[str, ...] = ()
    current_state: str = "unverified"
    complete: bool = False
    source_documents: tuple[dict, ...] = ()

    def export_step(self, path: str | Path, *, allow_partial: bool = False) -> dict:
        """Export STEP plus a JSON loss report after verifying an XDE round-trip.

        Exports the current CadQuery assembly, including caller-assigned leaf RGB
        colors. Native Inventor appearance and current state remain unverified.
        Existing outputs are never overwritten. Partial exports require opt-in.
        """
        from .assembly_step import export_step
        return export_step(self, path, allow_partial=allow_partial)


class AssemblyConversionError(ValueError):
    """An incomplete conversion, with the reviewable partial result attached."""
    def __init__(self, message: str, result: AssemblyConversion):
        super().__init__(message)
        self.result = result


@dataclass(frozen=True)
class InventorAssembly:
    summary: dict
    definitions: tuple[AssemblyDefinition, ...]
    instances: tuple[AssemblyInstance, ...]
    limits: Limits | None = None

    @property
    def structure_status(self) -> str:
        return self.summary["structure_status"]

    @property
    def complete(self) -> bool:
        return self.summary["complete"]

    def to_cadquery(self, *, allow_partial: bool = False,
                   allow_unverified_state: bool = False) -> AssemblyConversion:
        """Build hierarchy in mm; omissions are never silently discarded.

        Acknowledge saved-state uncertainty explicitly. ``allow_partial`` permits
        missing/unsupported geometry and returns its occurrence-level reasons.
        Native names are used when present; otherwise names are derived from the
        referenced filename. Unique occurrence suffixes preserve repeated parts.
        Native colors are not decoded. ``AssemblyConversion.export_step`` checks
        the hierarchy, names, placements and any caller-assigned leaf RGB colors.
        """
        if not allow_unverified_state:
            raise ValueError("Current Model State/visibility/substitute selection is unverified; "
                             "pass allow_unverified_state=True to convert saved placements")
        import cadquery as cq
        from OCP.gp import gp_Trsf
        from . import _file_bytes, read

        root = self.definitions[self.summary["root"]]
        assembly = cq.Assembly(name=Path(root.path).stem)
        nodes = {None: assembly}
        groups = []
        shapes = {}
        failed = {}
        omissions = []
        converted = 0

        def omit(index, instance, reason, detail):
            omissions.append(AssemblyOmission(index, instance.path, reason, detail))

        def location(matrix):
            # gp_Trsf can orthogonalize or remove scale. Check first so conversion
            # never silently changes a decoded matrix (including mirrored ones).
            for i in range(3):
                for j in range(3):
                    dot = sum(matrix[k][i] * matrix[k][j] for k in range(3))
                    if abs(dot - (1.0 if i == j else 0.0)) > 1e-10:
                        raise ValueError("non-rigid placement is unsupported")
            a, b, c = (row[:3] for row in matrix[:3])
            det = (a[0]*(b[1]*c[2]-b[2]*c[1]) - a[1]*(b[0]*c[2]-b[2]*c[0])
                   + a[2]*(b[0]*c[1]-b[1]*c[0]))
            if abs(det - 1.0) > 1e-10:
                raise ValueError("mirrored placement is unsupported")
            transform = gp_Trsf()
            transform.SetValues(*(x for row in matrix[:3] for x in row))
            return cq.Location(transform)

        for index, instance in enumerate(self.instances):
            if instance.suppressed is True:
                omit(index, instance, "suppressed", "stored reference suppression flag")
                continue
            if instance.visible is False:
                omit(index, instance, "hidden", "occurrence visibility is false")
                continue
            if instance.substitute is True:
                omit(index, instance, "unsupported_substitute", "substitute selection is not implemented")
                continue
            parent = nodes.get(instance.parent)
            if parent is None:
                omit(index, instance, "parent_unavailable", "parent occurrence was omitted")
                continue
            if instance.resolution != "resolved" or instance.definition is None:
                omit(index, instance, instance.resolution, "reference or placement is unresolved")
                continue
            definition = self.definitions[instance.definition]
            native_name = instance.name or Path(definition.path).stem
            name = f"{native_name.replace('/', '_')} [{instance.occurrence_id}@{instance.occurrence_index}]"
            try:
                loc = location(instance.local_transform_mm)
                if definition.document.summary["kind"] == "assembly":
                    nodes[index] = cq.Assembly(name=name, loc=loc)
                    groups.append((index, instance.parent))
                    continue
                if definition.key in failed:
                    raise ValueError(failed[definition.key])
                if definition.key not in shapes:
                    try:
                        data = _file_bytes(Path(definition.path), self.limits)
                        if hashlib.sha256(data).hexdigest() != definition.document.summary["source_sha256"]:
                            raise ValueError("source file changed after assembly resolution")
                        for alias in definition.source_paths:
                            if alias != definition.path and hashlib.sha256(_file_bytes(Path(alias), self.limits)).hexdigest() != definition.document.summary["source_sha256"]:
                                raise ValueError("source alias changed after assembly resolution: " + alias)
                        parts = read(data, source_id=definition.path, limits=self.limits).to_cadquery().vals()
                        if not parts:
                            raise ValueError("part conversion produced no shapes")
                        shapes[definition.key] = cq.Compound.makeCompound(parts)
                    except Exception as error:
                        failed[definition.key] = str(error)
                        raise
                parent.add(shapes[definition.key], name=name, loc=loc,
                           metadata={"definition_key": definition.key, "occurrence_path": instance.path})
                converted += 1
            except Exception as error:
                omit(index, instance, "geometry_unavailable", str(error))
        # CadQuery copies on add. Attach finished child assemblies bottom-up so
        # both traversal and the public objects dictionary contain descendants.
        for index, parent_index in reversed(groups):
            nodes[parent_index].add(nodes[index])
        reference_issues = tuple({"owner": d.key, **r} for d in self.definitions
                                for r in d.references if r["status"] != "resolved")
        diagnostics = tuple(self.summary["diagnostics"]) + tuple(
            f"{d.path}: {issue['code']}: {issue['message']}"
            for d in self.definitions for issue in d.document.diagnostics)
        sources = tuple(dict(path=d.path, source_paths=d.source_paths,
                             sha256=d.document.summary['source_sha256'], key=d.key,
                             document_id=d.document.summary['ufrx']['document_id'])
                        for d in self.definitions)
        result = AssemblyConversion(assembly, tuple(omissions), converted, len(shapes),
                                    reference_issues, diagnostics, source_documents=sources)
        if not allow_partial and (self.structure_status != "resolved" or
                                  any(o.reason not in ("suppressed", "hidden") for o in omissions)):
            raise AssemblyConversionError("Assembly conversion is incomplete; inspect result.omissions "
                                          "and the reference graph, or pass allow_partial=True", result)
        return result


@dataclass(frozen=True)
class FileSystemResolver:
    """Offline resolver: root parent plus explicit roots, without recursive search.

    Absolute Windows paths fall back to filenames within those roots. Case
    collisions, identity mismatches, cycles and symlinks leaving roots are explicit.
    Limits cover the entire load/expansion. Re-resolving creates a fresh snapshot.
    """
    search_roots: tuple[str | Path, ...] = ()
    max_documents: int = 256
    max_instances: int = 10000
    max_depth: int = 32
    max_total_file_bytes: int = 512 * 1024 * 1024
    max_directory_entries: int = 100000
    limits: Limits | None = None

    def read(self, path: str | Path) -> InventorAssembly:
        summary = json.loads(_inventor.resolve_assembly(str(path), [str(r) for r in self.search_roots],
            self.max_documents, self.max_instances, self.max_depth, self.max_total_file_bytes,
            self.max_directory_entries, _encoded_limits(self.limits)))
        definitions = tuple(AssemblyDefinition(d["key"], d["path"], _document(d["document"]),
                                                tuple(d["references"]), tuple(d["source_paths"])) for d in summary["definitions"])
        instances = tuple(AssemblyInstance(**{**i, "path": tuple(i["path"]),
            "local_transform_mm": _matrix(i["local_transform_mm"]),
            "world_transform_mm": _matrix(i["world_transform_mm"])}) for i in summary["instances"])
        return InventorAssembly(summary, definitions, instances, self.limits)


def read_assembly_file(path: str | Path, *, search_roots=(), **limits) -> InventorAssembly:
    """Resolve saved IAM structure; geometry conversion is an explicit second step."""
    return FileSystemResolver(tuple(search_roots), **limits).read(path)
