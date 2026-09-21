import copy
from contextlib import redirect_stdout
import io
import json
import math
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace as Obj
import unittest
from unittest.mock import patch

from jsonschema import ValidationError

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import capture_drawing_oracle as capture
from drawing_oracle_contract import identity, load_capture, read_json, readiness, validate_capture
import validate_drawing_oracle as validator


class Collection:
    def __init__(self, *values, broken=None):
        self.values, self.Count, self.broken = values, len(values), broken
    def Item(self, index):
        if index == self.broken:
            raise RuntimeError('Item unavailable')
        return self.values[index-1]


def point(x, y):
    return Obj(X=x, Y=y)


def fake_capture(root):
    """Entirely synthetic COM-shaped objects; no native IDW correctness claim."""
    line = Obj(StartPoint=point(1.,2.), EndPoint=point(4.,6.))
    layer = Obj(Name='Visible')
    segment = Obj(GeometryType=5251, Geometry=line, Visible=True, HiddenLine=False, Layer=layer)
    view = Obj(Name='VIEW1', ViewType=10501, Position=point(10.,12.), Scale=.5, Rotation=0., Width=5., Height=4.,
               DrawingCurves=Collection(Obj(Segments=Collection(segment))), Sketches=Collection())
    style = Obj(Font='Synthetic Font', FontSize=.35)
    text = Obj(Text='図面 ⌀10\nline', FormattedText='図面 ⌀10\nline', Position=point(2.,3.), Origin=point(2.,3.),
               Rotation=0., Height=.7, Width=3., TextStyle=style, Style=style)
    entity = Obj(Type=83896064, Geometry=line, Construction=False, Layer=layer)
    sketch = Obj(Name='Sketch1', Visible=True, SketchEntities=Collection(entity), TextBoxes=Collection(text),
                 SketchToSheetSpace=lambda p: point(p.X+1., p.Y+2.))
    sheet = Obj(Name='Sheet:1', Width=29.7, Height=21., Orientation=10243, Size=9993, Status=0,
                DrawingViews=Collection(view), Sketches=Collection(sketch), DrawingNotes=Obj(GeneralNotes=Collection(text)),
                DrawingDimensions=Collection(Obj(Type=117474560, Text=Obj(Text='10',FormattedText='10',Origin=point(5.,7.)))),
                PartsLists=Collection(Obj(Title='PARTS LIST',Position=point(4.,8.),PartsListRows=Collection(Collection(Obj(Value='1'),Obj(Value='部品A'))))),
                Border=None, TitleBlock=Obj(Name='Title', Definition=Obj(Sketch=Obj(TextBoxes=Collection(text))),GetResultText=lambda t:'Evaluated title'))
    doc = Obj(DocumentType=12292, Dirty=False, RequiresUpdate=False, DrawingSettings=Obj(DeferUpdates=True), Sheets=Collection(sheet),
              File=Obj(ReferencedFileDescriptors=Collection()))
    app = Obj(SoftwareVersion=Obj(DisplayName='Fake provider', BuildIdentifier='test'),
              TransientGeometry=Obj(CreatePoint2d=point))
    source = root/'synthetic.idw'
    source.write_bytes(b'synthetic data, NOT a native IDW')
    data = capture.collect(app, doc, identity(source), root,
                           [dict(path=source.name,identity=identity(source))], 'autodesk.apprentice')
    data['provider'].update(name='synthetic',platform='test')
    data['capture'].update(scope='synthetic',acquired_at='2026-09-16T00:00:00Z')
    return data, source, app, doc


class DrawingOracle(unittest.TestCase):
    def test_synthetic_collection_retains_coordinates_text_and_evaluated_cells(self):
        with tempfile.TemporaryDirectory() as temporary:
            data, source, app, doc = fake_capture(Path(temporary))
            validate_capture(data, source=source)
            sheet = data['sheets']['items'][0]['result']['value']
            self.assertEqual(sheet['width']['value'],29.7)
            view = sheet['views']['items'][0]['result']['value']
            segment = view['curves']['items'][0]['result']['value']['segments']['items'][0]['result']['value']
            self.assertEqual(segment['geometry']['value'],dict(kind='line_segment',start=[1.,2.],end=[4.,6.]))
            self.assertIn('図面 ⌀10',sheet['notes']['items'][0]['result']['value']['text']['value'])
            self.assertEqual(sheet['sketches']['items'][0]['result']['value']['sheet_basis']['value'],[[1.,2.],[2.,2.],[1.,3.]])
            self.assertEqual(sheet['title_block']['value']['texts']['items'][0]['result']['value'],'Evaluated title')
            self.assertEqual(readiness(data)['status'],'incomplete')
            self.assertIn('synthetic_provider',readiness(data)['reasons'])
            self.assertEqual(data['capture']['automatic_update'],'unknown')

    def test_geometry_type_comes_from_owner_and_unsupported_curves_are_explicit(self):
        collector = capture.Collector(None, ROOT)
        circle = Obj(Center=point(2.,3.),Radius=4.)
        arc = Obj(Center=point(0.,0.), Radius=1., StartPoint=point(1.,0.),EndPoint=point(0.,1.),StartAngle=0.,SweepAngle=math.pi/2)
        self.assertEqual(collector.geometry(circle,5252)['radius'],4.)
        self.assertAlmostEqual(collector.geometry(arc,5253)['sweep_angle'],math.pi/2)
        result = capture.observe(lambda: collector.geometry(circle,5254))
        self.assertEqual(result['status'],'unavailable')
        self.assertNotIn('value',result)
        result = capture.observe(lambda: collector.geometry(None, 5251))
        self.assertEqual(result['status'], 'unavailable')
        self.assertNotIn('value', result)
        block = collector.block(Obj(Name='Default border', Definition=Obj(Sketch=None)))
        self.assertEqual(block['texts']['status'], 'unavailable')
        self.assertIsNone(block['texts']['reported_count'])

    def test_failed_properties_and_items_never_become_zero_or_empty(self):
        collector = capture.Collector(None, ROOT)
        self.assertEqual(capture.prop(Obj(),'Width',float)['status'],'failed')
        result = collector.collection(lambda: Collection('a','b',broken=2), lambda v:v)
        self.assertEqual(result['status'],'partial')
        self.assertEqual(result['reported_count'],2)
        self.assertEqual(result['items'][1]['result']['status'],'failed')
        self.assertEqual(collector.collection(lambda: None,lambda v:v)['status'],'unavailable')
        self.assertEqual(collector.collection(lambda: Obj(),lambda v:v)['reported_count'],None)
        self.assertEqual(collector.collection(lambda: Collection(),lambda v:v)['status'],'captured')
        collector.remaining=0
        self.assertEqual(collector.collection(lambda: Collection(1),lambda v:v)['status'],'failed')
        for value in (float('nan'), float('inf'), True):
            self.assertEqual(capture.prop(Obj(Width=value),'Width',float)['status'],'failed')

    def test_copy_capture_closes_only_its_provider_and_detects_mutation(self):
        # This exercises lifecycle with fake COM dispatch; it is not Windows evidence.
        for provider in ('apprentice','inventor'):
            for behavior in ('normal','open_error','mutate_copy'):
                with self.subTest(provider=provider,behavior=behavior),tempfile.TemporaryDirectory() as temporary:
                    root=Path(temporary)
                    _,source,app,doc=fake_capture(root)
                    events=[]
                    def open_doc(path,*options):
                        events.append(('open',path,options))
                        if behavior=='open_error':
                            raise RuntimeError('fake open failed')
                        if behavior=='mutate_copy':
                            Path(path).write_bytes(b'changed')
                        return doc
                    app.Open=open_doc
                    app.Close=lambda:events.append(('apprentice_close',))
                    app.Quit=lambda:events.append(('quit',))
                    doc.Close=lambda skip:events.append(('document_close',skip))
                    options=Obj(Add=lambda name,value:events.append(('option',name,value)))
                    app.TransientObjects=Obj(CreateNameValueMap=lambda:options)
                    app.Documents=Obj(OpenWithOptions=open_doc)
                    client=Obj(DispatchEx=lambda progid:app)
                    with patch.dict(sys.modules,{'win32com':Obj(client=client),'win32com.client':client}),patch.object(capture.sys,'platform','win32'):
                        if behavior=='normal':
                            data=capture.capture(source,provider,root)
                            self.assertEqual(data['capture']['automatic_update'],'unknown')
                        else:
                            with self.assertRaisesRegex((RuntimeError,ValueError),'fake open failed|changed during capture'):
                                capture.capture(source,provider,root)
                    self.assertEqual(source.read_bytes(),b'synthetic data, NOT a native IDW')
                    if provider=='apprentice':
                        self.assertIn(('apprentice_close',),events)
                        self.assertNotIn(('quit',),events)
                    else:
                        self.assertIn(('quit',),events)
                        if behavior!='open_error':
                            self.assertIn(('document_close',True),events)
                        self.assertIn(('option','DeferUpdates',True),events)
                        self.assertIn(('option','SkipAllUnresolvedFiles',True),events)

    def test_contract_rejects_status_count_order_nonfinite_and_source_mismatches(self):
        with tempfile.TemporaryDirectory() as temporary:
            data, source, _, _ = fake_capture(Path(temporary))
            mutations=[lambda d:d['sheets'].update(reported_count=0),
                       lambda d:d['sheets']['items'][0].update(index=2),
                       lambda d:d['sheets'].update(status='partial'),
                       lambda d:d['sheets']['items'][0]['result']['value']['width'].update(value=float('inf')),
                       lambda d:d['sheets']['items'][0]['result']['value']['width'].update(status='failed'),
                       lambda d:d['capture'].update(automatic_update='not_occurred'),
                       lambda d:d['source'].update(sha256='0'*64),
                       lambda d:d['provider'].update(name='autodesk.apprentice')]
            for mutate in mutations:
                with self.subTest(mutation=mutate):
                    changed=copy.deepcopy(data)
                    mutate(changed)
                    with self.assertRaises((ValueError,ValidationError)):
                        validate_capture(changed,source=source)

    def test_real_query_failures_still_form_a_valid_incomplete_capture(self):
        with tempfile.TemporaryDirectory() as temporary:
            data, source, app, doc = fake_capture(Path(temporary))
            del doc.Sheets.Item(1).Width
            doc.Sheets.Item(1).DrawingViews.broken=1
            data=capture.collect(app,doc,identity(source),temporary,data['input_files'],'autodesk.apprentice')
            validate_capture(data,source=source)
            self.assertEqual(readiness(data)['status'],'incomplete')
            self.assertEqual(data['sheets']['items'][0]['result']['value']['width']['status'],'failed')

    def test_oversized_reported_count_does_not_drive_readiness_allocation(self):
        with tempfile.TemporaryDirectory() as temporary:
            data,source,_,_=fake_capture(Path(temporary))
            data['sheets']=dict(status='failed',reported_count=10**30,items=[],reason='Budget exceeded')
            validate_capture(data,source=source)
            self.assertEqual(readiness(data)['status'],'incomplete')

    def test_staged_references_have_hashes_and_external_ones_are_unavailable(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            data, source, _, _=fake_capture(root)
            collector=capture.Collector(None,root)
            ref=collector.reference(Obj(FullFileName=str(source),ReferenceMissing=False))
            self.assertEqual(ref['identity']['value'],identity(source))
            ref=collector.reference(Obj(FullFileName=str(ROOT/'README.md'),ReferenceMissing=False))
            self.assertEqual(ref['identity']['status'],'unavailable')

    def test_offline_validation_checks_staged_dependency_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            data,source,_,_=fake_capture(root)
            part=root/'part.ipt';part.write_bytes(b'synthetic part')
            data['input_files'].append(dict(path=part.name,identity=identity(part)))
            validate_capture(data,source=source)
            part.write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError,'Staged input identity mismatch'):
                validate_capture(data,source=source)

    def test_project_staging_preserves_relative_paths_and_bounds(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            project=root/'project'
            (project/'parts').mkdir(parents=True)
            source=project/'one.idw'; source.write_bytes(b'fake')
            (project/'parts/one.ipt').write_bytes(b'fake part')
            copied, original_root, files=capture.stage_project(source,project,root/'copy')
            self.assertEqual(copied.read_bytes(),b'fake')
            self.assertEqual({v['path'] for v in files},{'one.idw','parts/one.ipt'})
            self.assertEqual((root/'copy/parts/one.ipt').read_bytes(),b'fake part')
            with patch.object(capture,'MAX_PROJECT_BYTES',1), self.assertRaisesRegex(ValueError,'staging limit'):
                capture.stage_project(source,project,root/'oversize')
            with self.assertRaisesRegex(ValueError,'outside project'):
                capture.stage_project(source,project,project/'nested')

    def test_visuals_are_bound_to_source_and_actual_payloads(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary)
            data,source,_,_=fake_capture(root)
            pdf=root/'sheet.pdf'; pdf.write_bytes(b'%PDF-1.7\nsynthetic placeholder')
            meta=identity(pdf)
            artifact=dict(path=pdf.name,sha256=meta['sha256'],bytes=meta['bytes'],media_type='application/pdf',
                source_sha256=data['source']['sha256'],sheet_index=1,binding='unverified',provenance='Synthetic test bytes only',
                dpi=capture.unavailable('PDF vector units'),fonts=capture.unavailable('Not collected'))
            data['visual_artifacts']=[artifact]
            validate_capture(data,source=source,artifact_root=root)
            for key,value in [('path','../outside.pdf'),('sha256','0'*64),('sheet_index',2),('source_sha256','0'*64)]:
                changed=copy.deepcopy(data);changed['visual_artifacts'][0][key]=value
                with self.subTest(key=key),self.assertRaises((ValueError,FileNotFoundError)):
                    validate_capture(changed,source=source,artifact_root=root)

    def test_json_rejects_duplicate_keys_and_nonfinite_literals(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'bad.json'
            for value in ('{"a":1,"a":2}','{"a":NaN}','{"a":Infinity}'):
                path.write_text(value)
                with self.assertRaises(ValueError):
                    read_json(path)

    def test_missing_or_synthetic_native_evidence_fails_default_gate(self):
        with patch.object(sys,'argv',['validator']),patch.object(validator,'inventory',return_value=dict(acquisition='incomplete')),redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                validator.main()
            self.assertEqual(error.exception.code,1)
        with patch.object(sys,'argv',['validator','--inventory']),patch.object(validator,'inventory',return_value=dict(acquisition='incomplete')),redirect_stdout(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                validator.main()
            self.assertEqual(error.exception.code,0)
        with tempfile.TemporaryDirectory() as temporary:
            data,source,_,_=fake_capture(Path(temporary))
            path=Path(temporary)/'capture.json';path.write_text(json.dumps(data))
            args=['validator','--capture',str(path),'--source',str(source),'--require-ready']
            with patch.object(sys,'argv',args),redirect_stdout(io.StringIO()),self.assertRaises(SystemExit) as error:
                validator.main()
            self.assertEqual(error.exception.code,1)

    def test_checked_in_synthetic_fixture_is_explicitly_non_native(self):
        data=load_capture(ROOT/'tests/data/drawing-oracle-synthetic.json')
        self.assertEqual(data['provider']['name'],'synthetic')
        self.assertEqual(readiness(data)['status'],'incomplete')


if __name__ == '__main__':
    unittest.main()
