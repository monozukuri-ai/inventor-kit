"""Collect bounded drawing observations on Windows; no Save/Update calls.

Not yet qualified on an Autodesk installation. Missing API access is explicit.
DrawingCurveSegment geometry is sheet-space; sketch geometry remains local with
three basis points queried through SketchToSheetSpace. Database units: cm/rad.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import shutil
import sys
import tempfile

from drawing_oracle_contract import MAX_JSON_BYTES, identity, validate_capture

MAX_ITEMS = 100_000
MAX_PROJECT_BYTES = 512 * 1024 * 1024
MAX_PROJECT_FILES = 10_000


class Unavailable(Exception):
    pass


def observe(query):
    try:
        return dict(status='captured', value=query())
    except Unavailable as error:
        return dict(status='unavailable', reason=str(error))
    except Exception as error:
        return dict(status='failed', reason=f'{type(error).__name__}: {error}'[:2000])


def unavailable(reason):
    return dict(status='unavailable', reason=reason)


def scalar(value, kind):
    if kind is float:
        if type(value) in (float, int) and math.isfinite(value):
            return float(value)
        raise TypeError('Expected a finite number')
    if type(value) is kind:
        return value
    raise TypeError(f'Expected {kind.__name__}, received {type(value).__name__}')


def point(value):
    return [scalar(value.X, float), scalar(value.Y, float)]


def prop(value, name, convert=str):
    return observe(lambda: point(getattr(value, name)) if convert is point else scalar(getattr(value, name), convert))


class Collector:
    def __init__(self, app, staged_root):
        self.app, self.staged_root = app, Path(staged_root).resolve()
        self.remaining = MAX_ITEMS

    def collection(self, query, collect):
        count = None
        try:
            objects = query()
            if objects is None:
                raise Unavailable('Provider returned Nothing, not an observed empty collection')
            reported = scalar(objects.Count, int)
            if reported < 0:
                raise ValueError('Negative collection count')
            count = reported
            if count > self.remaining:
                raise ValueError('Collection exceeds capture item budget')
            self.remaining -= count
        except Exception as error:
            return dict(status='unavailable' if isinstance(error, Unavailable) else 'failed',
                        reported_count=count, items=[], reason=f'{type(error).__name__}: {error}'[:2000])
        values = [dict(index=i, result=observe(lambda i=i: collect(objects.Item(i)))) for i in range(1, count + 1)]
        return dict(status='captured' if all(v['result']['status'] == 'captured' for v in values) else 'partial',
                    reported_count=count, items=values)

    def geometry(self, value, kind):
        # Transient LineSegment2d/Circle2d/Arc2d objects have no Type property.
        # The owner supplies Curve2dTypeEnum, or a documented sketch type map.
        if value is None:
            raise Unavailable('Provider returned Nothing for geometry; absence semantics are unverified')
        kind = scalar(kind, int)
        if kind == 5251:  # kLineSegmentCurve2d
            return dict(kind='line_segment', start=point(value.StartPoint), end=point(value.EndPoint))
        if kind in (5252, 5253):  # circle / circular arc
            result = dict(kind='circle' if kind == 5252 else 'arc', center=point(value.Center), radius=scalar(value.Radius, float))
            if kind == 5253:
                result.update(start=point(value.StartPoint), end=point(value.EndPoint),
                              start_angle=scalar(value.StartAngle, float), sweep_angle=scalar(value.SweepAngle, float))
            return result
        raise Unavailable(f'Curve2d type {kind} is outside line/circle/arc capture profile')

    def segment(self, value):
        return dict(geometry_type=prop(value, 'GeometryType', int), visible=prop(value, 'Visible', bool),
                    hidden_line=prop(value, 'HiddenLine', bool), layer=observe(lambda: scalar(value.Layer.Name, str)),
                    geometry=observe(lambda: self.geometry(value.Geometry, value.GeometryType)))

    def text(self, value, position='Position', style='TextStyle'):
        return dict(text=prop(value, 'Text'), formatted_text=prop(value, 'FormattedText'), position=prop(value, position, point),
                    rotation=prop(value, 'Rotation', float), height=prop(value, 'Height', float), width=prop(value, 'Width', float),
                    font=observe(lambda: scalar(getattr(value, style).Font, str)),
                    font_size=observe(lambda: scalar(getattr(value, style).FontSize, float)),
                    range_min=observe(lambda: point(value.RangeBox.MinPoint)),
                    range_max=observe(lambda: point(value.RangeBox.MaxPoint)),
                    horizontal_justification=prop(value, 'HorizontalJustification', int),
                    vertical_justification=prop(value, 'VerticalJustification', int))

    def sketch(self, value):
        def basis():
            return [point(value.SketchToSheetSpace(self.app.TransientGeometry.CreatePoint2d(x, y)))
                    for x, y in ((0., 0.), (1., 0.), (0., 1.))]
        def entity(entity):
            def geometry():
                kind = {83896064: 5251, 83899648: 5252, 83898880: 5253}.get(entity.Type)
                if kind is None:
                    raise Unavailable(f'Sketch object type {entity.Type} is outside line/circle/arc profile')
                return self.geometry(entity.Geometry, kind)
            return dict(object_type=prop(entity, 'Type', int), construction=prop(entity, 'Construction', bool),
                        layer=observe(lambda: scalar(entity.Layer.Name, str)), geometry=observe(geometry))
        return dict(name=prop(value, 'Name'), visible=prop(value, 'Visible', bool), coordinate_space='sketch',
                    sheet_basis=observe(basis), entities=self.collection(lambda: value.SketchEntities, entity),
                    texts=self.collection(lambda: value.TextBoxes, lambda box: self.text(box, 'Origin', 'Style')))

    def view(self, value):
        result = dict(name=prop(value, 'Name'), view_type=prop(value, 'ViewType', int), position=prop(value, 'Position', point),
                      scale=prop(value, 'Scale', float), rotation=prop(value, 'Rotation', float),
                      width=prop(value, 'Width', float), height=prop(value, 'Height', float))
        result['curves'] = self.collection(lambda: value.DrawingCurves,
            lambda curve: dict(segments=self.collection(lambda: curve.Segments, self.segment)))
        result['sketches'] = self.collection(lambda: value.Sketches, self.sketch)
        return result

    def dimension(self, value):
        return dict(object_type=prop(value, 'Type', int), text=observe(lambda: scalar(value.Text.Text, str)),
                    formatted_text=observe(lambda: scalar(value.Text.FormattedText, str)),
                    position=observe(lambda: point(value.Text.Origin)))

    def block(self, value):
        if value is None:
            return None
        def text_boxes():
            sketch = value.Definition.Sketch
            return None if sketch is None else sketch.TextBoxes
        return dict(name=prop(value, 'Name'), texts=self.collection(text_boxes,
                    lambda box: scalar(value.GetResultText(box), str)))

    def parts_list(self, value):
        return dict(title=prop(value, 'Title'), position=prop(value, 'Position', point),
                    rows=self.collection(lambda: value.PartsListRows,
                        lambda row: self.collection(lambda: row, lambda cell: scalar(cell.Value, str))))

    def sheet(self, value):
        return dict(name=prop(value, 'Name'), width=prop(value, 'Width', float), height=prop(value, 'Height', float),
                    orientation=prop(value, 'Orientation', int), size=prop(value, 'Size', int), sheet_status=prop(value, 'Status', int),
                    views=self.collection(lambda: value.DrawingViews, self.view), sketches=self.collection(lambda: value.Sketches, self.sketch),
                    notes=self.collection(lambda: value.DrawingNotes.GeneralNotes, self.text),
                    dimensions=self.collection(lambda: value.DrawingDimensions, self.dimension),
                    parts_lists=self.collection(lambda: value.PartsLists, self.parts_list),
                    border=observe(lambda: self.block(value.Border)), title_block=observe(lambda: self.block(value.TitleBlock)))

    def reference(self, value):
        def reference_identity():
            target = Path(value.FullFileName).resolve()
            if not target.is_relative_to(self.staged_root):
                raise Unavailable('Resolved reference lies outside staged project; identity not collected')
            return identity(target)
        return dict(full_file_name=prop(value, 'FullFileName'), missing=prop(value, 'ReferenceMissing', bool),
                    identity=observe(reference_identity))


def document_state(doc):
    return dict(document_type=prop(doc, 'DocumentType', int), dirty=prop(doc, 'Dirty', bool),
                requires_update=prop(doc, 'RequiresUpdate', bool),
                defer_updates=observe(lambda: scalar(doc.DrawingSettings.DeferUpdates, bool)))


def stage_project(source, project_root, destination):
    """Bounded copy preserving relative references; refuse links/escaped input."""
    source, destination = Path(source).resolve(), Path(destination).resolve()
    root = Path(project_root).resolve() if project_root else source.parent
    if not source.is_relative_to(root):
        raise ValueError('Input must be inside project root')
    if destination == root or (project_root and destination.is_relative_to(root)):
        raise ValueError('Staging directory must be outside project root')
    paths = root.rglob('*') if project_root else [source]
    files, total, entries = [], 0, 0
    for path in paths:
        entries += 1
        if entries > MAX_PROJECT_FILES * 2:
            raise ValueError('Project exceeds staging limit (directory entries)')
        if path.is_symlink():
            raise ValueError('Project contains a symbolic link')
        if path.is_dir():
            continue
        if not path.is_file():
            raise ValueError('Project contains a non-regular file')
        total += path.stat().st_size
        if total > MAX_PROJECT_BYTES or len(files) >= MAX_PROJECT_FILES:
            raise ValueError('Project exceeds staging limit')
        files.append(dict(path=path.relative_to(root).as_posix(), identity=identity(path)))
    files.sort(key=lambda entry: entry['path'])
    for entry in files:
        target = destination / entry['path']
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / entry['path'], target)
        if identity(target) != entry['identity']:
            raise ValueError('Input changed during staging')
    return destination / source.relative_to(root), root, files


def collect(app, doc, source_identity, staged_root, files, provider):
    collector = Collector(app, staged_root)
    before = document_state(doc)
    sheets = collector.collection(lambda: doc.Sheets, collector.sheet)
    references = collector.collection(lambda: doc.File.ReferencedFileDescriptors, collector.reference)
    return dict(schema_version=1, source=source_identity,
        provider=dict(name=provider, version=observe(lambda: scalar(app.SoftwareVersion.DisplayName, str) + ' / ' + str(app.SoftwareVersion.BuildIdentifier)),
                      platform='Windows', collector_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()),
        capture=dict(acquired_at=datetime.now(timezone.utc).isoformat(), scope='opened_document', source_preserved=True,
                     save_requested=False, update_requested=False, automatic_update='unknown',
                     state_evidence=unavailable('No independent confirmation of opening-time update/migration'),
                     open_options=['DeferUpdates=True', 'SkipAllUnresolvedFiles=True', 'OpenVisible=False'] if provider == 'autodesk.inventor' else []),
        units=dict(length='cm', angle='rad', drawing_curves='sheet'), input_files=files,
        document_state_before=before, document_state_after=document_state(doc), sheets=sheets, references=references,
        visual_artifacts=[], limitations=[
            'Collector has not been qualified on a Windows/Autodesk installation.',
            'Opened document observations are not proof of the original saved drawing state.',
            'Reference descriptors are direct references, not proof of a complete transitive dependency closure.',
            'Only line, circle and circular arc geometry is decoded; unsupported queries retain status/reason.',
            'Sketch basis points are provider observations; affine behavior has not been independently qualified.',
            'Leader notes, symbols, hatches, images, border/title-block geometry and table layout are outside this capture profile.',
            'No PDF/PNG export or font installation audit is performed.'])


def capture(path, provider='apprentice', project_root=None):
    if sys.platform != 'win32':
        raise RuntimeError('Capture requires Windows and Autodesk; validation is available offline with validate_drawing_oracle.py')
    import win32com.client
    path = Path(path).resolve()
    if path.suffix.lower() != '.idw':
        raise ValueError('Expected an IDW input')
    original = identity(path)
    with tempfile.TemporaryDirectory(prefix='inventor-drawing-oracle-') as temporary:
        copied, root, files = stage_project(path, project_root, temporary)
        app, doc = None, None
        try:
            # Use a separate COM instance, never attach to the interactive application.
            app = win32com.client.DispatchEx('Inventor.ApprenticeServerComponent' if provider == 'apprentice' else 'Inventor.Application')
            if provider == 'apprentice':
                doc = app.Open(str(copied))
            else:
                options = app.TransientObjects.CreateNameValueMap()
                options.Add('DeferUpdates', True)
                options.Add('SkipAllUnresolvedFiles', True)
                doc = app.Documents.OpenWithOptions(str(copied), options, False)
            if doc.DocumentType != 12292:
                raise ValueError('Provider did not identify a drawing document')
            data = collect(app, doc, original, temporary, files, 'autodesk.' + provider)
        finally:
            try:
                if app is not None and provider == 'apprentice':
                    app.Close()
                elif doc is not None:
                    doc.Close(True)  # SkipSave
            finally:
                if app is not None and provider == 'inventor':
                    app.Quit()
        for entry in files:
            if identity(root / entry['path']) != entry['identity'] or identity(Path(temporary) / entry['path']) != entry['identity']:
                raise ValueError('Source/project changed during capture')
    return validate_capture(data, source=path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--provider', choices=('apprentice','inventor'), default='apprentice')
    parser.add_argument('--project-root', type=Path, help='Copy only this bounded project tree, preserving relative reference paths')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Output already exists; choose a new capture path')
    data = capture(args.input, args.provider, args.project_root)
    payload = (json.dumps(data, indent=2, ensure_ascii=False, allow_nan=False)+'\n').encode('utf-8')
    if len(payload) > MAX_JSON_BYTES:
        raise ValueError('Capture exceeds JSON size limit')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('xb') as output:
        output.write(payload)
    print('Capture written. Saved-state binding, visuals and Windows qualification remain separate gates.')


if __name__ == '__main__':
    main()
