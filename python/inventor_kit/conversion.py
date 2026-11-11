"""Body-scoped saved geometry, explicit selection, and serializable diagnostics.

Geometry is owned by cq-acis. This module never fills missing faces or chooses a
primary body implicitly. Importing it does not import the geometry libraries.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, field
import hashlib
from importlib.metadata import PackageNotFoundError, version
import json
import math
import re
from typing import Any


def dependency_versions():
    result = {}
    for name in ("inventor-kit", "cq-acis", "cadquery", "cadquery-ocp", "cadquery-ocp-novtk"):
        try:
            result[name] = version(name)
        except PackageNotFoundError:
            result[name] = None
    return result


@dataclass(frozen=True)
class ConversionDiagnostic:
    code: str
    message: str
    severity: str = "error"
    source: dict | None = None
    entity: int | None = None
    entity_type: str | None = None
    subtype: str | None = None


@dataclass(frozen=True)
class BodyConversion:
    id: str
    body_index: int
    source: dict | None
    status: str
    diagnostics: tuple[ConversionDiagnostic, ...] = ()
    metrics: dict | None = None
    #: A CadQuery ``Shape`` from the CadQuery backend, an OCCT ``TopoDS_Shape``
    #: from the OCCT backend.
    shape: Any = field(default=None, repr=False, compare=False)
    #: Warnings about a usable result, such as stored B-spline fits that stand
    #: in for procedural geometry. A warned body is derived, not exact.
    warnings: tuple[ConversionDiagnostic, ...] = ()
    #: The stored fits behind the warning, as recorded by cq-acis.
    approximations: tuple[dict, ...] = field(default=(), repr=False, compare=False)

    def report(self):
        return dict(id=self.id, body_index=self.body_index, source=self.source,
                    status=self.status, diagnostics=[asdict(d) for d in self.diagnostics],
                    metrics=self.metrics, warnings=[asdict(d) for d in self.warnings])


class BodyConversionError(ValueError):
    """A refused selection with its complete, inspectable conversion result."""

    def __init__(self, message, result):
        super().__init__(message)
        self.result = result


@dataclass(frozen=True)
class PartConversion:
    source_sha256: str | None
    candidate_id: str | None
    bodies: tuple[BodyConversion, ...]
    diagnostics: tuple[ConversionDiagnostic, ...] = ()
    #: Shape backend of the bodies: "cadquery" or "occt"; None when nothing was converted.
    backend: str | None = None
    current_state_verified: bool = field(default=False, init=False)

    @property
    def geometry_complete(self):
        return bool(self.bodies) and not self.diagnostics and all(
            b.status == "converted_solid" for b in self.bodies)

    @property
    def complete(self):
        return self.geometry_complete and self.current_state_verified

    def _requested(self, body_ids):
        if body_ids is None:
            return self.bodies
        if isinstance(body_ids, (str, bytes)):
            raise ValueError("body_ids must be a nonempty sequence of body IDs")
        ids = tuple(body_ids)
        if not ids or any(not isinstance(i, str) for i in ids) or len(set(ids)) != len(ids):
            raise ValueError("body_ids must contain unique body IDs")
        known = {b.id: b for b in self.bodies}
        if any(i not in known for i in ids):
            raise ValueError("Unknown body ID for this input and selected candidate")
        # Preserve source order even when callers list the IDs in another order.
        return tuple(b for b in self.bodies if b.id in ids)

    def report(self, *, body_ids=None):
        requested = self._requested(body_ids)
        selected = {b.id for b in requested}
        usable = tuple(b for b in requested if b.status == "converted_solid")
        selection_complete = bool(requested) and len(usable) == len(requested) and not self.diagnostics
        omissions = [dict(body_id=b.id, body_index=b.body_index,
                          reason="not_selected" if b.id not in selected else b.status,
                          diagnostics=[asdict(d) for d in b.diagnostics])
                     for b in self.bodies if b.id not in selected or b.status != "converted_solid"]
        if usable:
            status = "success" if not omissions and selection_complete else "partial"
        else:
            status = "error" if any(b.status == "failed" for b in requested) else "unsupported"
        return dict(schema_version=1, kind="part", status=status, units="mm",
                    source_sha256=self.source_sha256, candidate_id=self.candidate_id,
                    geometry_complete=self.geometry_complete, selection_complete=selection_complete,
                    current_state_verified=False, complete=False,
                    requested_body_ids=[b.id for b in requested],
                    converted_body_ids=[b.id for b in usable],
                    bodies=[b.report() for b in self.bodies], omissions=omissions,
                    diagnostics=[asdict(d) for d in self.diagnostics], dependencies=dependency_versions())

    def selected_bodies(self, *, body_ids=None, allow_partial=False):
        """Return usable selected bodies; every omission needs explicit opt-in."""
        report = self.report(body_ids=body_ids)
        if not report["converted_body_ids"]:
            raise BodyConversionError("No selected body produced a valid solid", self)
        if not allow_partial and report["status"] != "success":
            raise BodyConversionError("Partial body selection requires allow_partial=True", self)
        return tuple(b for b in self._requested(body_ids) if b.status == "converted_solid")

    def _require_cadquery(self, operation):
        if self.backend == "occt":
            raise ValueError(f"{operation} needs CadQuery shapes; convert with "
                             "convert_bodies(backend='cadquery') or use to_occt()")

    def to_occt(self, *, body_ids=None, allow_partial=False):
        """Return the selected solid bodies as OCCT ``TopoDS_Shape`` objects."""
        selected = self.selected_bodies(body_ids=body_ids, allow_partial=allow_partial)
        return tuple(getattr(b.shape, "wrapped", b.shape) for b in selected)

    def to_cadquery(self, *, body_ids=None, allow_partial=False):
        self._require_cadquery("to_cadquery")
        selected = self.selected_bodies(body_ids=body_ids, allow_partial=allow_partial)
        import cadquery as cq
        return cq.Workplane("XY").newObject([b.shape for b in selected])

    def export_step(self, path, *, body_ids=None, allow_partial=False):
        """Export selected solid bodies and a checked provenance/omission report."""
        self._require_cadquery("export_step")
        selected = self.selected_bodies(body_ids=body_ids, allow_partial=allow_partial)
        import cadquery as cq
        from .assembly_step import write_step
        assembly = cq.Assembly(name="Part")
        for body in selected:
            assembly.add(body.shape, name=f"Body_{body.body_index}")
        selection = self.report(body_ids=body_ids)
        return write_step(assembly, path, dict(kind="part", conversion=selection,
            source_documents=[dict(sha256=self.source_sha256, candidate_id=self.candidate_id)],
            omissions=selection["omissions"], reference_issues=[],
            diagnostics=selection["diagnostics"], current_state="unverified", complete=False,
            geometry_complete=self.geometry_complete, selection_complete=selection["selection_complete"],
            current_state_verified=False, name_origin="source body index",
            color_origin="not_decoded", vendor_comparison="not_collected"))


def _source(entity):
    raw = getattr(entity, "raw", entity)
    span = getattr(raw, "source", None)
    return asdict(span) if span is not None else None


def _failure(error, model, body):
    # cq-acis currently exposes a code and text, not an entity reference. Only
    # attach a text-referenced index when it resolves in this exact model.
    match = re.search(r"\$(\d+)", str(error))
    entity = model.resolve(int(match[1])) if match else None
    raw = getattr(entity, "raw", entity)
    geometry = None
    for attribute in ("curve", "surface"):
        reference = getattr(entity, attribute, None)
        if reference is not None:
            geometry = model.resolve(reference)
            break
    geometry = getattr(geometry, "raw", geometry)
    subtype = next((v for v in getattr(geometry, "values", ()) if isinstance(v, str)), None)
    return ConversionDiagnostic(getattr(error, "code", "geometry.conversion_failed"), str(error),
        source=_source(entity) if entity is not None else _source(body),
        entity=getattr(raw, "index", None), entity_type=getattr(raw, "type_name", None), subtype=subtype)


def _approximation_warning(approximations, model, body):
    """One warning per body that depends on stored B-spline fits."""
    subtypes = sorted({a.get("subtype") or a.get("type") or "unknown" for a in approximations})
    tolerance = max((a.get("fit_tolerance_mm") or 0.0) for a in approximations)
    return ConversionDiagnostic(
        "geometry.approximated_from_stored_spline",
        f"body ${body.index}: {len(approximations)} stored B-spline fit(s) stand in for procedural "
        f"geometry ({', '.join(subtypes)}); largest fit tolerance {tolerance:.6g} mm",
        severity="warning", source=_source(body))


def convert_bodies(document, backend="auto"):
    if document.metadata is not None and document.metadata.stages.geometry == "not_attempted":
        raise ValueError("Geometry was not requested; use read or read_file")
    geometry = document.geometry
    sha256 = geometry.source_sha256 if geometry is not None else None
    candidate = geometry.selection.selected_id if geometry is not None else None
    if document.model is None:
        reasons = document.summary.get("diagnostics", ())
        return PartConversion(sha256, candidate, (), (ConversionDiagnostic(
            "geometry.unavailable", "; ".join(reasons) or "No stored geometry was selected"),))
    if not sha256 or not candidate:
        raise ValueError("Body conversion requires input hash and selected candidate provenance")
    from cq_acis import BACKENDS, BodyEntity, CadQueryConverter, CadQueryConversionError, RawEntity
    from .assembly_step import _metrics
    if backend not in BACKENDS:
        raise ValueError(f"unknown backend {backend!r}; expected one of {BACKENDS}")
    results, resolved = [], None
    # Include undecoded raw bodies; a partial model must never look complete.
    bodies = [e for e in document.model.entities if isinstance(e, BodyEntity)
              or (isinstance(e, RawEntity) and e.type_name == "body")]
    for body in bodies:
        identity = json.dumps([sha256, candidate, body.index], separators=(",", ":"))
        body_id = "body-" + hashlib.sha256(identity.encode()).hexdigest()
        shape, metrics, diagnostics, warnings, approximations = None, None, (), (), ()
        try:
            if not isinstance(body, BodyEntity):
                raise CadQueryConversionError(f"body ${body.index}: body has not been decoded",
                                             code="geometry.body_unsupported")
            # One converter per body prevents a failed attempt from leaking
            # mutable OCCT caches into another body's result.
            converter = CadQueryConverter(document.model, backend=backend)
            resolved = converter.backend
            shape = converter.convert_body(body)
            metrics = _metrics(shape)
            if not metrics["valid"] or not metrics["solids"] or not math.isfinite(metrics["volume_mm3"]) or metrics["volume_mm3"] <= 0:
                raise CadQueryConversionError(f"body ${body.index}: invalid or non-positive solid",
                                             code="geometry.body_invalid")
            json.dumps(metrics, allow_nan=False)
            status = "converted_solid"
            approximations = tuple(dict(a) for a in getattr(converter, "approximations", ()))
            if approximations:
                warnings = (_approximation_warning(approximations, document.model, body),)
            if resolved == "occt":
                shape = shape.wrapped
        except CadQueryConversionError as error:
            status = ("open_shell" if error.code == "geometry.shell_open" else
                      "unsupported" if error.code.endswith("unsupported") or error.code == "geometry.sewing_no_shell"
                      else "failed")
            shape, metrics = None, None
            diagnostics = (_failure(error, document.model, body),)
        results.append(BodyConversion(body_id, body.index, _source(body), status, diagnostics, metrics, shape,
                                      warnings, approximations))
    diagnostics = () if results else (ConversionDiagnostic("geometry.no_bodies", "The stored table contains no bodies"),)
    return PartConversion(sha256, candidate, tuple(results), diagnostics, resolved)
