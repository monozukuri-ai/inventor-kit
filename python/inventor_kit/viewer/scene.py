"""Build display snapshots without changing parser or saved-state semantics."""
from dataclasses import asdict, dataclass, fields, is_dataclass
import hashlib
import json
import math
from pathlib import Path

from .. import Limits, _file_bytes, inspect, read


@dataclass(frozen=True)
class Options:
    quality: str = "normal"
    metadata_only: bool = False
    candidate_id: str | None = None
    require_current_state: bool = False
    limits: Limits = Limits()
    timeout: float = 120.0
    max_triangles: int = 2_000_000
    max_buffer_bytes: int = 128 * 1024 * 1024
    search_roots: tuple[str, ...] = ()
    allow_unverified_state: bool = False
    allow_partial: bool = False

    def __post_init__(self):
        if self.quality not in ("draft", "normal", "fine"):
            raise ValueError("Unknown mesh quality")
        if not math.isfinite(self.timeout) or self.timeout <= 0:
            raise ValueError("Timeout must be finite and positive")
        for name in ("max_triangles", "max_buffer_bytes"):
            if type(getattr(self, name)) is not int or getattr(self, name) < 0:
                raise ValueError(f"{name} must be a nonnegative integer")
        if self.metadata_only and (self.candidate_id or self.require_current_state):
            raise ValueError("Selection options require geometry")
        if self.metadata_only and (self.search_roots or self.allow_unverified_state or self.allow_partial):
            raise ValueError("Assembly options cannot be combined with metadata-only")
        if self.allow_partial and not self.allow_unverified_state:
            raise ValueError("--allow-partial requires --allow-unverified-state")
        if (self.search_roots or self.allow_unverified_state or self.allow_partial) and (self.candidate_id or self.require_current_state):
            raise ValueError("Assembly options cannot be combined with IPT selection options")


def empty_scene(name):
    return dict(schema_version=1, job_status="queued", source=dict(name=name, sha256=None, kind="unknown"),
                units="mm", current_state="unverified", complete=False,
                stages={key: "not_attempted" for key in ("metadata", "geometry", "conversion", "tessellation")},
                selection=None, candidates=[], nodes=[], meshes=[], properties=[], thumbnails=[],
                diagnostics=[], tessellation=None, assembly=None, omissions=[], reference_issues=[])


def discard_geometry(scene, status="not_displayed", reason="Geometry was not published"):
    """Keep the IAM inventory when a worker or conversion fails."""
    scene["meshes"] = []
    scene["tessellation"] = None
    if scene["assembly"] is None:
        scene["nodes"] = []
        return
    scene["assembly"].update(displayed_instances=0, displayed_definitions=0)
    for node in scene["nodes"]:
        if node["mesh_id"] or node["status"] in ("pending", "displayable"):
            node.update(status=status, reason=reason)
        node["mesh_id"] = None


def diagnostic(scene, code, message, severity="error", source=None):
    scene["diagnostics"].append(dict(code=code, message=message, severity=severity, source=source))


def safe_value(value):
    """Text values preserve integer/Decimal precision; never expose raw bytes."""
    if isinstance(value, bytes):
        return f"<{len(value)} bytes>"
    if is_dataclass(value):
        return {f.name: safe_value(getattr(value, f.name)) for f in fields(value)}
    if isinstance(value, (tuple, list)):
        return [safe_value(v) for v in value]
    if isinstance(value, dict):
        return {str(k): safe_value(v) for k, v in value.items()}
    if value is None or isinstance(value, (str, bool)):
        return value
    return str(value)


def write_scene(directory, scene, name="state.json"):
    data = json.dumps(scene, ensure_ascii=False, allow_nan=False).encode("utf-8")
    if len(data) > 16 * 1024 * 1024:
        raise ValueError("Viewer metadata exceeds 16 MiB")
    target = directory / name
    temporary = directory / (name + ".tmp")
    temporary.write_bytes(data)
    temporary.replace(target)


def build_scene(path, directory, options, publish=lambda scene: None):
    """Use one input snapshot for metadata, candidates and shape construction."""
    path, directory = Path(path), Path(directory)
    scene = empty_scene(path.name)
    scene["job_status"] = "running"
    stage = "metadata"
    try:
        data = _file_bytes(path, options.limits)
        scene["source"]["sha256"] = hashlib.sha256(data).hexdigest()
        doc = inspect(data, source_id=path.name, include_candidates=not options.metadata_only, limits=options.limits)
        scene["source"]["kind"] = doc.metadata.identification.kind
        scene["stages"]["metadata"] = "available"
        for prop_set in doc.metadata.property_sets:
            for p in prop_set.properties:
                scene["properties"].append(dict(name=p.name or p.semantic_name or f"PID {p.pid}",
                    semantic_name=p.semantic_name, fmtid=p.fmtid, pid=p.pid, value_kind=p.value_kind,
                    value=safe_value(p.value), status=p.status, source=asdict(p.source),
                    storage_scope=p.storage_scope, state_binding=p.state_binding))
        total_preview_bytes = 0
        for index, thumb in enumerate(doc.metadata.thumbnails):
            total_preview_bytes += len(thumb.data)
            if index >= 64 or total_preview_bytes > 32 * 1024 * 1024:
                diagnostic(scene, "viewer.preview_limit", "Additional saved previews exceeded the display limit.", "warning")
                break
            name = f"preview-{index}.png"
            (directory / name).write_bytes(thumb.data)
            scene["thumbnails"].append(dict(resource=name, width=thumb.width, height=thumb.height,
                source=asdict(thumb.source), state_binding=thumb.state_binding, validation=thumb.validation))
        scene["diagnostics"] = [asdict(d) for d in doc.metadata.diagnostics] + scene["diagnostics"]
        scene["candidates"] = [dict(id=c.id, table_status=c.table_status, profile_status=c.profile_status,
            state_binding=c.state_binding, source=asdict(c.record_source)) for c in doc.geometry.candidates]
        publish(scene)
        if options.metadata_only:
            scene["job_status"] = "finished"
            return scene
        stage = "geometry"
        kind = doc.metadata.identification.kind
        if kind != "assembly" and (options.search_roots or options.allow_unverified_state or options.allow_partial):
            raise ValueError("Search roots and assembly permission options apply to IAM assemblies only")
        if kind == "assembly":
            if options.candidate_id or options.require_current_state:
                raise ValueError("Candidate and current-state options apply to IPT parts only")
            from .assembly import build_assembly_scene
            return build_assembly_scene(path, directory, options, scene, publish)
        if doc.metadata.identification.kind != "part":
            scene["stages"][stage] = "unsupported"
            diagnostic(scene, "viewer.document_kind", "3D viewing supports IPT parts and saved IAM assemblies. Document information is available.", "warning")
            if options.candidate_id or options.require_current_state:
                diagnostic(scene, "viewer.selection_kind", "Candidate and current-state options apply to IPT parts only.")
            scene["job_status"] = "finished"
            return scene
        doc = read(data, source_id=path.name, candidate_id=options.candidate_id,
                   require_current_state=options.require_current_state, limits=options.limits)
        scene["selection"] = asdict(doc.geometry.selection)
        scene["stages"][stage] = doc.summary["status"]
        scene["diagnostics"].extend(asdict(d) for d in doc.geometry.diagnostics)
        if doc.model is None:
            diagnostic(scene, "viewer.geometry_unavailable", "; ".join(doc.summary["diagnostics"]) or "No saved geometry was selected.")
            scene["job_status"] = "finished"
            return scene
        publish(scene)
        stage = "conversion"
        shapes = doc.to_cadquery().vals()
        if not shapes or any(not s.isValid() for s in shapes):
            raise ValueError("Saved geometry did not produce valid shapes")
        scene["stages"][stage] = "available"
        publish(scene)
        stage = "tessellation"
        from .tessellation import build_meshes
        nodes, meshes, settings = build_meshes(shapes, doc, directory, options)
        scene.update(nodes=nodes, meshes=meshes, tessellation=settings)
        scene["stages"][stage] = "available"
    except Exception as error:
        scene["stages"][stage] = "failed"
        diagnostic(scene, getattr(error, "code", f"viewer.{stage}_failed"), str(error))
        discard_geometry(scene)
    scene["job_status"] = "finished"
    return scene
