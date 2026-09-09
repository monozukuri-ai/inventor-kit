"""Checked STEP transport for M5; no claim of native appearance/state equivalence.

XDE is deliberately used instead of the flattened CadQuery STEP importer. The
comparison starts from the supplied CadQuery tree, independently of the exporter.
Only leaf shapes, rigid placements and leaf RGB colors are admitted. No topology
or geometry ownership moves out of the shared ACIS/Rust pipeline.
"""
from __future__ import annotations

from dataclasses import asdict
import hashlib
import json
import math
from pathlib import Path
import tempfile
import threading

_STEP_LOCK = threading.Lock()  # OCCT STEP unit/settings are process-global.
_IDENTITY = ((1., 0., 0., 0.), (0., 1., 0., 0.),
             (0., 0., 1., 0.), (0., 0., 0., 1.))


def _matrix(location):
    t = location.Transformation()
    if not math.isfinite(t.ScaleFactor()) or abs(t.ScaleFactor() - 1) > 1e-10:
        raise ValueError('STEP validation requires rigid placements without scale or reflection')
    return [[t.Value(i, j) for j in range(1, 5)] for i in range(1, 4)] + [[0., 0., 0., 1.]]


def _multiply(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def _metrics(shape):
    box = shape.BoundingBox()
    return dict(valid=shape.isValid(), solids=len(shape.Solids()), faces=len(shape.Faces()),
                volume_mm3=shape.Volume(), area_mm2=shape.Area(),
                bbox_mm=[getattr(box, k) for k in ('xmin', 'ymin', 'zmin', 'xmax', 'ymax', 'zmax')])


def _shape(node):
    import cadquery as cq
    if isinstance(node.obj, cq.Shape):
        return node.obj
    if isinstance(node.obj, cq.Workplane):
        values = node.obj.vals()
        if values and all(isinstance(s, cq.Shape) for s in values):
            return cq.Compound.makeCompound(values)
    raise ValueError('STEP supports only nonempty Shape/Workplane leaves')


def _snapshot(assembly):
    """Read the input tree, not an XDE document made by the exporter."""
    from OCP.TopLoc import TopLoc_Location
    import cadquery as cq
    nodes = []
    definitions = {}

    def visit(node, path, parent_matrix, parent_location, ancestors):
        if len(nodes) >= 10000 or len(path) > 64:
            raise ValueError('STEP occurrence/depth limit exceeded')
        if id(node) in ancestors:
            raise ValueError('cyclic CadQuery tree')
        if not node.name or '/' in node.name or '\x00' in node.name:
            raise ValueError('STEP node names must be nonempty and contain no slash/NUL')
        if node.material or node._subshape_colors or node._subshape_names or node._subshape_layers:
            raise ValueError('STEP validation does not support materials or subshape metadata')
        if node.children and (node.obj is not None or node.color is not None):
            raise ValueError('STEP validation requires geometry and colors on leaves only')
        path = (*path, node.name)
        matrix = _matrix(node.loc.wrapped)
        world = _multiply(parent_matrix, matrix)
        location = parent_location.Multiplied(node.loc.wrapped)
        color = list(node.color.toTuple()) if node.color else None
        if color is not None:
            if len(color) != 4 or not all(math.isfinite(v) and 0 <= v <= 1 for v in color):
                raise ValueError('invalid STEP RGBA color')
            if abs(color[3] - 1) > 1e-7:
                raise ValueError('STEP transparency round-trip is not supported')
            color = color[:3]
        entry = dict(path=path, kind='assembly' if node.children else 'part',
                     local_transform_mm=matrix, world_transform_mm=world, color_rgb=color,
                     definition=None, geometry=None)
        if not node.children:
            shape = _shape(node)
            # CadQuery's STEP exporter caches by object and appearance. Repeated
            # instances of the same shape/color must remain shared definitions.
            key = (id(node.obj), tuple(color) if color else None)
            entry['definition'] = definitions.setdefault(key, len(definitions))
            entry['geometry'] = _metrics(shape.moved(cq.Location(location)))
            if not entry['geometry']['valid'] or not shape.Solids():
                raise ValueError('STEP M5 validation requires valid solid leaves')
        nodes.append(entry)
        names = [child.name for child in node.children]
        if len(names) != len(set(names)):
            raise ValueError('duplicate sibling STEP names')
        for child in node.children:
            visit(child, path, world, location, ancestors | {id(node)})

    if not assembly.children or assembly.obj is not None:
        raise ValueError('STEP export requires an assembly containing at least one part')
    if not assembly.loc.wrapped.IsIdentity():
        raise ValueError('STEP export requires an identity root placement; place a child assembly instead')
    visit(assembly, (), _IDENTITY, TopLoc_Location(), set())
    # Validate finite matrices/metrics before touching the output filesystem.
    json.dumps(nodes, allow_nan=False)
    return nodes


def _read_xde(path):
    import cadquery as cq
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.Interface import Interface_Static
    from OCP.Quantity import Quantity_ColorRGBA, Quantity_TOC_sRGB
    from OCP.STEPCAFControl import STEPCAFControl_Reader
    from OCP.TCollection import TCollection_ExtendedString, TCollection_AsciiString
    from OCP.TDataStd import TDataStd_Name
    from OCP.TDF import TDF_Label, TDF_LabelSequence, TDF_Tool
    from OCP.TDocStd import TDocStd_Document
    from OCP.TopLoc import TopLoc_Location
    from OCP.XCAFDoc import XCAFDoc_DocumentTool, XCAFDoc_ColorGen, XCAFDoc_ColorSurf

    doc = TDocStd_Document(TCollection_ExtendedString('XmlXCAF'))
    reader = STEPCAFControl_Reader()
    reader.SetNameMode(True)
    reader.SetColorMode(True)
    Interface_Static.SetCVal_s('xstep.cascade.unit', 'MM')
    if reader.ReadFile(str(path)) != IFSelect_RetDone or not reader.Transfer(doc):
        raise ValueError('STEP XDE read/transfer failed')
    shapes = XCAFDoc_DocumentTool.ShapeTool_s(doc.Main())
    colors = XCAFDoc_DocumentTool.ColorTool_s(doc.Main())
    roots = TDF_LabelSequence()
    shapes.GetFreeShapes(roots)
    if roots.Length() != 1:
        raise ValueError('STEP must contain exactly one root assembly')
    nodes = []

    def label_id(label):
        result = TCollection_AsciiString()
        TDF_Tool.Entry_s(label, result)
        return result.ToCString()

    def name(label):
        value = TDataStd_Name()
        return value.Get().ToExtString() if label.FindAttribute(TDataStd_Name.GetID_s(), value) else ''

    def visit(label, path, parent_matrix, parent_location, ancestors):
        if len(nodes) >= 10000 or len(path) > 64:
            raise ValueError('STEP XDE occurrence/depth limit exceeded')
        definition = TDF_Label()
        if not shapes.GetReferredShape_s(label, definition):
            definition = label
        key = label_id(definition)
        if key in ancestors:
            raise ValueError('STEP XDE definition cycle')
        label_name = name(label) or name(definition)
        path = (*path, label_name)
        loc = shapes.GetLocation_s(label)
        matrix = _matrix(loc)
        world = _multiply(parent_matrix, matrix)
        location = parent_location.Multiplied(loc)
        is_group = shapes.IsAssembly_s(definition)
        color = None
        for target in (label, definition):
            for kind in (XCAFDoc_ColorSurf, XCAFDoc_ColorGen):
                value = Quantity_ColorRGBA()
                if colors.GetColor_s(target, kind, value):
                    color = list(value.GetRGB().Values(Quantity_TOC_sRGB))
                    break
            if color is not None:
                break
        entry = dict(path=path, kind='assembly' if is_group else 'part',
                     local_transform_mm=matrix, world_transform_mm=world, color_rgb=color,
                     definition=None if is_group else key, geometry=None)
        if not is_group:
            shape = cq.Shape.cast(shapes.GetShape_s(definition))
            entry['geometry'] = _metrics(shape.moved(cq.Location(location)))
        nodes.append(entry)
        if is_group:
            children = TDF_LabelSequence()
            shapes.GetComponents_s(definition, children, False)
            for i in range(1, children.Length() + 1):
                visit(children.Value(i), path, world, location, ancestors | {key})

    visit(roots.Value(1), (), _IDENTITY, TopLoc_Location(), set())
    json.dumps(nodes, allow_nan=False)
    return nodes


def _write_xde(assembly, path):
    from cadquery.occ_impl.assembly import toCAF
    from OCP.IFSelect import IFSelect_RetDone
    from OCP.Interface import Interface_Static
    from OCP.STEPCAFControl import STEPCAFControl_Writer
    from OCP.STEPControl import STEPControl_AsIs
    from OCP.TCollection import TCollection_ExtendedString
    from OCP.TDataStd import TDataStd_Name
    from OCP.TDF import TDF_Label, TDF_LabelSequence
    from OCP.XCAFDoc import XCAFDoc_DocumentTool

    root, doc = toCAF(assembly, coloredSTEP=True)
    shapes = XCAFDoc_DocumentTool.ShapeTool_s(doc.Main())

    def name_groups(label):
        children = TDF_LabelSequence()
        shapes.GetComponents_s(label, children, False)
        for i in range(1, children.Length() + 1):
            child = children.Value(i)
            definition = TDF_Label()
            if shapes.GetReferredShape_s(child, definition) and shapes.IsAssembly_s(definition):
                # CadQuery 2.8 leaves the subassembly *instance* label unnamed.
                # STEP readers then synthesize numeric names, losing hierarchy
                # paths. Its subassembly definition has the supplied node name.
                value = TDataStd_Name()
                if not definition.FindAttribute(TDataStd_Name.GetID_s(), value):
                    raise ValueError('missing subassembly definition name')
                TDataStd_Name.Set_s(child, TCollection_ExtendedString(value.Get().ToExtString()))
                name_groups(definition)

    definition = TDF_Label()
    name_groups(definition if shapes.GetReferredShape_s(root, definition) else root)
    writer = STEPCAFControl_Writer()
    writer.SetNameMode(True)
    writer.SetColorMode(True)
    Interface_Static.SetCVal_s('xstep.cascade.unit', 'MM')
    Interface_Static.SetCVal_s('write.step.unit', 'MM')
    Interface_Static.SetIVal_s('write.surfacecurve.mode', 1)
    Interface_Static.SetIVal_s('write.precision.mode', 0)
    if not writer.Transfer(doc, STEPControl_AsIs) or writer.Write(str(path)) != IFSelect_RetDone:
        raise ValueError('STEP XDE write/transfer failed')


def _compare(expected, actual):
    """Compare by complete name path, never by transfer order or shape count alone."""
    issues = []
    left = {tuple(n['path']): n for n in expected}
    right = {tuple(n['path']): n for n in actual}
    if len(left) != len(expected) or len(right) != len(actual):
        issues.append('duplicate occurrence paths')
    if left.keys() != right.keys():
        issues.append('hierarchy/names differ')

    def close(a, b, atol, rtol=0.):
        if isinstance(a, (tuple, list)):
            return isinstance(b, (tuple, list)) and len(a) == len(b) and all(close(x, y, atol, rtol) for x, y in zip(a, b))
        return isinstance(b, (float, int)) and math.isfinite(a) and math.isfinite(b) and math.isclose(a, b, abs_tol=atol, rel_tol=rtol)

    for path in left.keys() & right.keys():
        a, b = left[path], right[path]
        for field in ('kind',):
            if a[field] != b[field]:
                issues.append(f'{path}: {field} differs')
        for field in ('local_transform_mm', 'world_transform_mm'):
            if not close(a[field], b[field], 1e-7):
                issues.append(f'{path}: {field} differs')
        if (a['color_rgb'] is None) != (b['color_rgb'] is None) or (a['color_rgb'] is not None and not close(a['color_rgb'], b['color_rgb'], 2e-6)):
            issues.append(f'{path}: color differs')
        if a['geometry'] is None or b['geometry'] is None:
            if a['geometry'] != b['geometry']:
                issues.append(f'{path}: geometry presence differs')
            continue
        for field in ('valid', 'solids', 'faces'):
            if a['geometry'][field] != b['geometry'][field] or (field == 'valid' and not b['geometry'][field]):
                issues.append(f'{path}: {field} differs or invalid')
        for field, atol, rtol in (('volume_mm3', 1e-5, 1e-7), ('area_mm2', 1e-5, 1e-7), ('bbox_mm', 1e-5, 0.)):
            if not close(a['geometry'][field], b['geometry'][field], atol, rtol):
                issues.append(f'{path}: {field} differs')

    def partition(nodes):
        groups = {}
        for node in nodes:
            if node['kind'] == 'part':
                groups.setdefault(node['definition'], set()).add(tuple(node['path']))
        return {frozenset(paths) for paths in groups.values()}

    if partition(expected) != partition(actual):
        issues.append('shared part definitions differ')
    return dict(status='passed' if not issues else 'failed', issues=sorted(issues),
                scope='cadquery_to_step_xde', occurrences=len(actual) - 1,
                part_instances=sum(n['kind'] == 'part' for n in actual),
                part_definitions=len(partition(actual)),
                tolerances=dict(transform_absolute=1e-7, rgb_absolute=2e-6,
                                bbox_mm_absolute=1e-5, volume_mm3_absolute=1e-5,
                                area_mm2_absolute=1e-5, metric_relative=1e-7))


def export_step(conversion, path, *, allow_partial=False):
    """Write two new artifacts only after the complete STEP check succeeds."""
    from OCP.Interface import Interface_Static
    path = Path(path)
    sidecar = path.with_suffix(path.suffix + '.json')
    if path.suffix.lower() not in ('.step', '.stp'):
        raise ValueError('STEP output must end with .step or .stp')
    if path.exists() or sidecar.exists():
        raise FileExistsError('STEP output or JSON sidecar already exists')
    if not allow_partial and (conversion.reference_issues or conversion.diagnostics or
                              any(o.reason not in ('suppressed', 'hidden') for o in conversion.omissions)):
        raise ValueError('Partial STEP export requires allow_partial=True; inspect conversion losses')
    expected = _snapshot(conversion.assembly)
    with tempfile.TemporaryDirectory(prefix='inventor-step-') as temporary:
        staged = Path(temporary) / 'assembly.step'
        with _STEP_LOCK:
            # Restore the global OCCT parameters changed by CadQuery's exporter.
            # External OCCT calls from other threads still require caller locking.
            from OCP.STEPCAFControl import STEPCAFControl_Writer
            from OCP.XCAFDoc import XCAFDoc_ShapeTool
            STEPCAFControl_Writer()  # initialize STEP parameter definitions
            strings = {k: Interface_Static.CVal_s(k) for k in ('xstep.cascade.unit', 'write.step.unit')}
            ints = {k: Interface_Static.IVal_s(k) for k in ('write.surfacecurve.mode', 'write.precision.mode', 'write.stepcaf.subshapes.name')}
            auto_names = XCAFDoc_ShapeTool.AutoNaming_s()
            try:
                _write_xde(conversion.assembly, staged)
                actual = _read_xde(staged)
            finally:
                for k, v in strings.items():
                    Interface_Static.SetCVal_s(k, v)
                for k, v in ints.items():
                    Interface_Static.SetIVal_s(k, v)
                XCAFDoc_ShapeTool.SetAutoNaming_s(auto_names)
        comparison = _compare(expected, actual)
        if comparison['status'] != 'passed':
            raise ValueError('STEP round-trip failed: ' + '; '.join(comparison['issues']))
        data = staged.read_bytes()
        report = dict(schema_version=1, step_file=path.name, step_sha256=hashlib.sha256(data).hexdigest(),
                      bytes=len(data), units='mm', roundtrip=comparison, expected=expected, actual=actual,
                      source_documents=conversion.source_documents, omissions=[asdict(o) for o in conversion.omissions],
                      reference_issues=conversion.reference_issues, diagnostics=conversion.diagnostics,
                      current_state=conversion.current_state, complete=False,
                      name_origin='cadquery_input; native name or derived filename plus occurrence suffix',
                      color_origin='cadquery_input; native Inventor appearance not decoded',
                      vendor_comparison='not_collected')
        report_data = (json.dumps(report, indent=2, ensure_ascii=False, allow_nan=False) + '\n').encode('utf-8')
        # Exclusive creation prevents clobbering either an earlier report or CAD.
        # Roll back only files this call created if either write fails.
        created = []
        try:
            for output, contents in ((path, data), (sidecar, report_data)):
                with output.open('xb') as target:
                    created.append(output)
                    target.write(contents)
        except BaseException:
            for output in created:
                output.unlink()
            raise
        return report
