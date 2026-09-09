"""Read-only Apprentice capture on Windows; schema validation works offline anywhere."""
from datetime import datetime, timezone
from pathlib import Path
import argparse
import base64
import hashlib
import json
import shutil
import sys
import tempfile

from oracle_contract import load_capture, validate_capture


def items(collection):
    for i in range(1, collection.Count + 1):
        yield collection.Item(i)


def property_value(value):
    if value is None or type(value) in (str, int, float, bool):
        return value
    if isinstance(value, datetime):
        return value.isoformat()
    if isinstance(value, bytes):
        return {'base64': base64.b64encode(value).decode('ascii')}
    if isinstance(value, (list, tuple)):
        return [property_value(v) for v in value]
    raise TypeError(f'Unsupported COM variant: {type(value).__name__}')


def collect_document(doc):
    diagnostics, properties, references = [], [], []
    for group in items(doc.PropertySets):
        for prop in items(group):
            entry = dict(fmtid=group.InternalName, pid=prop.PropId, name=prop.Name,
                         value_type='unknown', value=None, status='unsupported')
            try:
                value = prop.Value
                entry.update(value_type=type(value).__name__, value=property_value(value), status='captured')
            except Exception as error:
                diagnostics.append(f'Property {group.InternalName}/{prop.PropId}: {error}')
            properties.append(entry)
    try:
        for ref in items(doc.File.ReferencedFileDescriptors):
            references.append(dict(full_file_name=ref.FullFileName, missing=bool(ref.ReferenceMissing)))
    except Exception as error:
        diagnostics.append(f'References unavailable or incomplete: {error}')
    kind = {12290: 'part', 12291: 'assembly', 12292: 'drawing'}.get(int(doc.DocumentType), 'other')
    return dict(kind=kind, internal_name=doc.InternalName, properties=properties,
                references=references, diagnostics=diagnostics)


def collect_geometry(doc):
    try:
        bodies = list(items(doc.ComponentDefinition.SurfaceBodies))
        if not bodies:
            raise ValueError('No surface bodies')
        metrics = []
        for body in bodies:
            # SurfaceBody.MassProperties was added in Inventor 2023. Older or
            # unavailable APIs report unavailable; no inferred mass properties.
            mass = body.MassProperties
            try:
                box, box_kind = body.PreciseRangeBox, 'precise'
            except Exception:
                box, box_kind = body.RangeBox, 'enclosing'
            metrics.append(dict(name=body.Name, solid=bool(body.IsSolid), faces=body.Faces.Count,
                                volume_mm3=float(mass.Volume) * 1000,
                                area_mm2=float(mass.Area) * 100,
                                bbox_mm=[float(getattr(p, axis)) * 10 for p in (box.MinPoint, box.MaxPoint) for axis in ('X', 'Y', 'Z')],
                                bbox_kind=box_kind))
        return dict(status='available', units='mm', body_count=len(metrics),
                    solid_count=sum(m['solid'] for m in metrics), faces=sum(m['faces'] for m in metrics),
                    volume_mm3=sum(m['volume_mm3'] for m in metrics), area_mm2=sum(m['area_mm2'] for m in metrics),
                    bbox_mm=[min(m['bbox_mm'][i] for m in metrics) for i in range(3)]
                            + [max(m['bbox_mm'][i] for m in metrics) for i in range(3, 6)],
                    bbox_kind='precise' if all(m['bbox_kind'] == 'precise' for m in metrics) else 'enclosing',
                    body_metrics=metrics)
    except Exception as error:
        return dict(status='unavailable', units='mm', reason=str(error))


def capture(path):
    if sys.platform != 'win32':
        raise RuntimeError('Capture requires Windows, pywin32 and Autodesk Apprentice; use --validate to check an existing JSON on other platforms')
    import win32com.client
    path = path.resolve()
    before = path.read_bytes()
    sha = hashlib.sha256(before).hexdigest()
    with tempfile.TemporaryDirectory(prefix='inventor-oracle-') as temporary:
        copied = Path(temporary) / path.name
        shutil.copy2(path, copied)
        app = win32com.client.Dispatch('Inventor.ApprenticeServerComponent')
        doc = app.Open(str(copied))
        try:
            data = dict(schema_version=1,
                        source=dict(sha256=sha, bytes=len(before), file_name=path.name),
                        provider=dict(name='autodesk.apprentice', version=app.SoftwareVersion.DisplayName + ' / ' + str(app.SoftwareVersion.BuildIdentifier), platform='Windows', collector_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()),
                        capture=dict(acquired_at=datetime.now(timezone.utc).isoformat(), scope='stored_document', model_state=None, source_modified=False, update_performed=False),
                        document=collect_document(doc), geometry=collect_geometry(doc),
                        limitations=['Apprentice factory/stored document only; no Model State activation or feature update.',
                                     'Reference resolution was observed on an isolated copy; referenced files were not copied.',
                                     'Database centimetres converted to mm; mass property precision is that of the provider.',
                                     'This collector must be qualified on a real Windows/Autodesk installation.'])
        finally:
            app.Close()
        if hashlib.sha256(copied.read_bytes()).hexdigest() != sha or hashlib.sha256(path.read_bytes()).hexdigest() != sha:
            raise ValueError('Source changed during oracle capture')
    return validate_capture(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--input', type=Path)
    group.add_argument('--validate', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.validate:
        load_capture(args.validate)
        print('Capture schema and invariants are valid; provider authenticity is not certified.')
        return
    if args.output is None:
        parser.error('--input requires --output')
    data = capture(args.input)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(data, ensure_ascii=False, indent=2, allow_nan=False)+'\n', encoding='utf-8')


if __name__ == '__main__':
    main()
