"""Bounded STEP/XDE occurrence-to-IPT batch writer for analytic development builds.

Each occurrence becomes a standalone world-coordinate part. No Inventor process,
assembly writer, geometry deduplication, or source STEP link is used.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import re
import sys
import tempfile
import unicodedata

from .writer import ANALYTIC_PROFILE, PartWriteError, _bounds, _name, new_part


@dataclass(frozen=True)
class StepPartLimits:
    max_file_bytes: int = 32 * 1024 * 1024
    max_entities: int = 250_000
    max_parts: int = 256
    max_nodes: int = 4096
    max_depth: int = 32
    max_total_topology: int = 32768
    max_output_bytes: int = 256 * 1024 * 1024

    def __post_init__(self):
        for name, value in asdict(self).items():
            if type(value) is not int or value < 1:
                raise ValueError(f'{name} must be a positive integer')


@dataclass(frozen=True)
class StepPartWriteReport:
    directory: Path
    report_path: Path
    part_count: int
    source_sha256: str
    native_validation: str = 'not_run'


class StepPartWriteError(PartWriteError):
    """Batch failure with a JSON-serializable ``report``; no batch was published."""
    def __init__(self, message, report):
        super().__init__(message)
        self.report = report


def _identity(path):
    data = path.read_bytes()
    return dict(path=path.name, bytes=len(data), sha256=hashlib.sha256(data).hexdigest())


def _metrics(shape):
    return dict(valid=shape.isValid(), solids=len(shape.Solids()), faces=len(shape.Faces()),
                edges=len(shape.Edges()), vertices=len(shape.Vertices()),
                volume_mm3=shape.Volume(), area_mm2=shape.Area(), bbox_mm=_bounds(shape))


def _matrix(location):
    t = location.Transformation()
    values = [[t.Value(i, j) for j in range(1, 5)] for i in range(1, 4)]
    if (not all(math.isfinite(x) for row in values for x in row)
            or abs(t.ScaleFactor() - 1) > 1e-10 or t.IsNegative()):
        raise PartWriteError('STEP placements must be finite, rigid and orientation preserving')
    return values + [[0., 0., 0., 1.]]


def _single_solid(shape):
    import cadquery as cq
    from OCP.TopAbs import TopAbs_COMPOUND, TopAbs_SOLID
    from OCP.TopoDS import TopoDS_Iterator
    solids = []
    pending = [shape.wrapped]
    while pending:
        item = pending.pop()
        if item.ShapeType() == TopAbs_SOLID:
            solids.append(cq.Shape.cast(item))
        elif item.ShapeType() == TopAbs_COMPOUND:
            iterator = TopoDS_Iterator(item)
            if not iterator.More():
                raise PartWriteError('empty STEP leaf compound')
            while iterator.More():
                pending.append(iterator.Value())
                iterator.Next()
        else:
            raise PartWriteError('STEP leaf includes non-solid geometry')
    if len(solids) != 1 or not solids[0].isValid():
        raise PartWriteError('each STEP occurrence must contain exactly one valid solid')
    return solids[0]


def _read_parts(path, limits):
    import cadquery as cq
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.STEPCAFControl import STEPCAFControl_Reader
    from OCP.TCollection import TCollection_AsciiString, TCollection_ExtendedString
    from OCP.TColStd import TColStd_SequenceOfAsciiString
    from OCP.TDataStd import TDataStd_Name
    from OCP.TDF import TDF_Label, TDF_LabelSequence, TDF_Tool
    from OCP.TDocStd import TDocStd_Document
    from OCP.TopLoc import TopLoc_Location
    from OCP.XCAFDoc import XCAFDoc_DocumentTool
    from .assembly_step import _STEP_LOCK

    def label_id(label):
        entry = TCollection_AsciiString()
        TDF_Tool.Entry_s(label, entry)
        return entry.ToCString()

    def label_name(label):
        value = TDataStd_Name()
        return value.Get().ToExtString() if label.FindAttribute(TDataStd_Name.GetID_s(), value) else ''

    with _STEP_LOCK:
        reader = STEPCAFControl_Reader()
        reader.SetNameMode(True)
        reader.SetColorMode(False)
        if reader.ReadFile(str(path)) != IFSelect_RetDone:
            raise PartWriteError('STEP parsing failed')
        if not reader.Reader().WS().ModelCheckList().IsEmpty(True):
            raise PartWriteError('STEP syntactic/semantic load failures')
        model = reader.Reader().StepModel()
        if model.NbEntities() > limits.max_entities:
            raise PartWriteError('STEP entity limit exceeded')
        # Reject external documents before Transfer can open referenced files.
        for i in range(1, model.NbEntities() + 1):
            if model.Value(i).IsKind('StepBasic_DocumentFile'):
                raise PartWriteError('external STEP documents are unsupported')
        length, angle, solid_angle = (TColStd_SequenceOfAsciiString() for _ in range(3))
        reader.Reader().FileUnits(length, angle, solid_angle)
        units = [length.Value(i).ToCString() for i in range(1, length.Length() + 1)]
        if units != ['millimetre']:
            raise PartWriteError(f'explicit millimetre STEP units required, got {units!r}')
        doc = TDocStd_Document(TCollection_ExtendedString('BinXCAF'))
        XCAFDoc_DocumentTool.SetLengthUnit_s(doc, 0.001)  # metres, independent of global defaults
        if not reader.Transfer(doc):
            raise PartWriteError('STEP XDE transfer failed')
        if not reader.Reader().WS().TransferReader().LastCheckList().IsEmpty(True):
            raise PartWriteError('STEP geometry transfer failures')
        shapes = XCAFDoc_DocumentTool.ShapeTool_s(doc.Main())
        roots = TDF_LabelSequence()
        shapes.GetFreeShapes(roots)
        parts, groups = [], []
        nodes = 0

        def visit(label, ancestry, labels, parent, ancestors):
            nonlocal nodes
            nodes += 1
            if nodes > limits.max_nodes or len(ancestry) >= limits.max_depth:
                raise PartWriteError('STEP occurrence/depth limit exceeded')
            definition = TDF_Label()
            if not shapes.GetReferredShape_s(label, definition):
                definition = label
            key = label_id(definition)
            if key in ancestors or shapes.IsExternRef_s(definition):
                raise PartWriteError('cyclic or external STEP definition')
            definition_name, occurrence_name = label_name(definition), label_name(label)
            name = definition_name or occurrence_name or f'part_{nodes:04d}'
            _name(name)
            located = shapes.GetShape_s(label)
            if located.IsNull():
                raise PartWriteError('STEP occurrence has no shape')
            # GetShape(label) already applies its occurrence placement. Only the
            # ancestor placement is added to leaf geometry (OCCT 7.9 GetShape).
            local = located.Location()
            world = parent.Multiplied(local)
            entry = dict(name=name, definition_name=definition_name, occurrence_name=occurrence_name,
                         name_inferred=not (definition_name or occurrence_name), definition=key,
                         path=[*ancestry, name], label_path=[*labels, label_id(label)],
                         local_transform_mm=_matrix(local), world_transform_mm=_matrix(world))
            if shapes.IsAssembly_s(definition):
                groups.append(entry)
                children = TDF_LabelSequence()
                shapes.GetComponents_s(definition, children, False)
                if not children.Length():
                    raise PartWriteError('empty STEP assembly')
                for i in range(1, children.Length() + 1):
                    visit(children.Value(i), entry['path'], entry['label_path'], world, ancestors | {key})
            else:
                if len(parts) >= limits.max_parts:
                    raise PartWriteError('STEP part limit exceeded')
                try:
                    shape = _single_solid(cq.Shape.cast(located).moved(cq.Location(parent)))
                except PartWriteError as exc:
                    raise PartWriteError(f'{name}: {exc}') from exc
                parts.append((entry, shape))

        for i in range(1, roots.Length() + 1):
            visit(roots.Value(i), [], [], TopLoc_Location(), set())
        if not parts:
            raise PartWriteError('STEP contains no solid occurrences')
        return parts, groups, units


def _filename(index, name):
    # The ordinal preserves repeated definitions and disambiguates names after
    # Windows-safe normalization. Original names remain in metadata/report.
    stem = re.sub(r'[<>:"/\\|?*\x00-\x1f]', '_', unicodedata.normalize('NFC', name))
    stem = stem.rstrip(' .') or 'part'
    while len(stem.encode('utf-16-le')) // 2 > 80:
        stem = stem[:-1]
    return f'{index:04d}_{stem}.ipt'


def _publish_directory(staging, target):
    """Atomically publish without replacing even an empty competing directory."""
    if sys.platform == 'win32':
        os.rename(staging, target)  # Windows refuses an existing destination.
        return
    libc = ctypes.CDLL(None, use_errno=True)
    if sys.platform.startswith('linux') and hasattr(libc, 'renameat2'):
        fn = libc.renameat2
        fn.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
        args = (-100, os.fsencode(staging), -100, os.fsencode(target), 1)  # RENAME_NOREPLACE
    elif sys.platform == 'darwin' and hasattr(libc, 'renamex_np'):
        fn = libc.renamex_np
        fn.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
        args = (os.fsencode(staging), os.fsencode(target), 4)  # RENAME_EXCL
    else:
        raise PartWriteError('exclusive directory publication is unavailable on this platform')
    fn.restype = ctypes.c_int
    if fn(*args) != 0:
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), str(target))


def _compare(actual, expected):
    a, e = _metrics(actual), _metrics(expected)
    json.dumps([a, e], allow_nan=False)
    if e['volume_mm3'] <= 0 or e['area_mm2'] <= 0:
        raise PartWriteError('STEP occurrence has nonpositive volume/area')
    if not a['valid'] or a['solids'] != 1 or any(a[k] != e[k] for k in ('faces', 'edges', 'vertices')):
        raise PartWriteError('IPT self-read topology differs from STEP occurrence')
    errors = {k: abs(a[k] - e[k]) / e[k] for k in ('volume_mm3', 'area_mm2')}
    bounds = max(abs(x-y) for x, y in zip(a['bbox_mm'], e['bbox_mm'], strict=True))
    differences = (expected.cut(actual), actual.cut(expected))
    if not all(shape.isValid() for shape in differences):
        raise PartWriteError('could not verify IPT geometry: boolean comparison produced an invalid shape')
    difference = sum(shape.Volume() for shape in differences)
    if (max(errors.values()) > 1e-6 or bounds > 1e-5 or not math.isfinite(difference)
            or difference < 0 or difference > e['volume_mm3'] * 1e-7):
        raise PartWriteError('IPT self-read geometry differs from STEP occurrence')
    return dict(status='passed', expected=e, observed=a, relative_errors=errors,
                bounds_max_error_mm=bounds, symmetric_difference_mm3=difference)


def write_step_parts(source: str | os.PathLike, output_directory: str | os.PathLike, *,
                     target_profile: str = ANALYTIC_PROFILE,
                     limits: StepPartLimits | None = None) -> StepPartWriteReport:
    """Write each metric STEP solid occurrence as a standalone IPT in a new directory.

    Preserve names and world geometry, without deduplication or appearance import.
    Validate the entire staged batch before an exclusive directory rename. Errors
    carry a diagnostic report; existing output paths (including symlinks) are never
    replaced. Per-output Inventor validation is not run. Requires CadQuery and the
    matching analytic writer. Input and output budgets are bounded.
    """
    limits = StepPartLimits() if limits is None else limits
    if not isinstance(limits, StepPartLimits):
        raise TypeError('limits must be StepPartLimits')
    new_part(name='batch', target_profile=target_profile)
    source, target = Path(source).absolute(), Path(output_directory).absolute()
    if os.path.lexists(target):
        raise FileExistsError(target)
    diagnostic = dict(status='failed', published=False, phase='input', part=None,
                      source=str(source), output_directory=str(target), native_validation='not_run')
    try:
        if source.stat().st_size > limits.max_file_bytes:
            raise PartWriteError('STEP file byte limit exceeded')
        with source.open('rb') as f:
            data = f.read(limits.max_file_bytes + 1)
        if len(data) > limits.max_file_bytes:
            raise PartWriteError('STEP file byte limit exceeded')
        digest = hashlib.sha256(data).hexdigest()
        diagnostic['source_sha256'] = digest
        with tempfile.TemporaryDirectory(prefix='.inventor-batch-', dir=target.parent) as tmp:
            scratch = Path(tmp)
            snapshot = scratch / 'source.step'
            snapshot.write_bytes(data)
            diagnostic['phase'] = 'inventory'
            parts, groups, units = _read_parts(snapshot, limits)
            total = sum(len(s.Faces()) + len(s.Edges()) + len(s.Vertices()) for _, s in parts)
            if total > limits.max_total_topology:
                raise PartWriteError('aggregate STEP topology limit exceeded')
            staging = scratch / 'parts'
            staging.mkdir()
            rows, filenames, output_bytes = [], set(), 0
            from . import read_file
            for index, (entry, shape) in enumerate(parts, 1):
                diagnostic.update(phase='encode', part=entry['path'])
                filename = _filename(index, entry['name'])
                if filename.casefold() in filenames:
                    raise PartWriteError('case-insensitive output filename collision')
                filenames.add(filename.casefold())
                part = new_part(name=entry['name'], title=entry['name'], part_number=entry['name'], target_profile=target_profile)
                part.add_body(shape, name=entry['name'], feature_name='Base feature')
                out = part.saveas(staging / filename)
                diagnostic['phase'] = 'self-read'
                comparison = _compare(read_file(out.path).to_cadquery().val(), shape)
                sidecar = json.loads(out.report_path.read_text(encoding='utf-8'))
                sidecar['artifact']['path'] = filename  # relocatable within the batch
                sidecar['source_occurrence'] = entry
                out.report_path.write_text(json.dumps(sidecar, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
                artifact, report = _identity(out.path), _identity(out.report_path)
                output_bytes += artifact['bytes'] + report['bytes']
                if output_bytes > limits.max_output_bytes:
                    raise PartWriteError('aggregate IPT output byte limit exceeded')
                rows.append(dict(**entry, artifact=artifact, report=report, geometry=comparison, native_validation='not_run'))
            diagnostic.update(phase='publish', part=None)
            with source.open('rb') as f:
                if f.read(limits.max_file_bytes + 1) != data:
                    raise PartWriteError('source STEP changed during batch generation')
            batch = dict(schema_version=1, status='local-validated', target_profile=target_profile,
                         source=dict(path=str(source), bytes=len(data), sha256=digest), units=units,
                         placement='world coordinates; occurrence location applied exactly once',
                         geometry_deduplication=False, appearance_imported=False, product_Inventor_API_dependency=False,
                         native_validation='not_run', limits=asdict(limits), parts=rows, assemblies=groups)
            payload = (json.dumps(batch, ensure_ascii=False, indent=2, allow_nan=False) + '\n').encode('utf-8')
            if output_bytes + len(payload) > limits.max_output_bytes:
                raise PartWriteError('aggregate output report byte limit exceeded')
            (staging / 'batch.json').write_bytes(payload)
            _publish_directory(staging, target)
        return StepPartWriteReport(target, target / 'batch.json', len(rows), digest)
    except Exception as exc:
        diagnostic['error'] = str(exc)
        raise StepPartWriteError(str(exc), diagnostic) from exc
