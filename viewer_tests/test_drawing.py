"""2D scene publication and resource lifetime through the normal worker/server."""
import json
import hashlib
import os
import queue
import signal
import subprocess
import sys
from pathlib import Path
import tempfile
import threading
import time
import unittest
from copy import deepcopy
from urllib.error import HTTPError
from urllib.request import urlopen

import jsonschema
from inventor_kit.viewer.scene import Options, build_scene, empty_scene, write_scene
from inventor_kit.viewer.server import create_server
from inventor_kit.viewer.worker import Job
from inventor_kit.viewer.drawing import _pack_styles, _pack_view_references, _pack_item_records, _unpack_styles, sheet_bytes, validate_drawing_resources

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT/'schemas/drawing-scene-v1.schema.json').read_text())


def drawing_failure(path, directory, options):
    scene = empty_scene('metadata survives')
    scene['source']['kind'] = 'drawing'
    scene['drawing'] = dict(images=[dict(resource='drawing-image-1.png')], sheets=[])
    (Path(directory)/'drawing-image-1.png').write_bytes(b'pending image')
    write_scene(Path(directory), scene)
    write_scene(Path(directory), scene, 'pending.json')
    os._exit(9)


class DrawingScenes(unittest.TestCase):
    def test_compact_items_roundtrip_exact_identity_geometry_and_source_spans(self):
        sheet = 'a'*64 + '/sheet-00000000-0000-0000-0000-000000000000-1'
        segment = '00000000-0000-0000-0000-000000000002'
        source = dict(source_id='日本語.idw',stream='/RSeStorage/Bdata',byte_domain='inflated_stream',
                      start_offset=10,end_offset=20)
        item = dict(id=f'{sheet}/{segment}/3/4',geometry=dict(kind='polyline',points=[[0.,1.,2.],[3.,4.,5.]]),
                    style=dict(visible=True,sources=[source]),source=source,segment_id=segment,
                    record_ordinal=4,placement_record=3,group_path=[0,3])
        second = dict(item,id=f'{sheet}/{segment}/3/5',record_ordinal=5,source=dict(source,start_offset=30,end_offset=40))
        view = dict(item_ids=[second['id'],item['id']])
        payload = dict(schema_version=1,sheet_id=sheet,items=[item,second],views=[view],
                       export_report=dict(sheets=[dict(views=[view])]))
        packed = _pack_item_records(_pack_view_references(_pack_styles(payload)))
        self.assertEqual(len(packed['placements']),1)
        self.assertEqual(len(packed['source_bases']),1)
        self.assertEqual(_unpack_styles(json.loads(sheet_bytes(packed))),payload)
        for index, value in [(0,1),(2,1),(1,True),(3,-1),(4,9),(1,9007199254740992)]:
            bad=deepcopy(packed);bad['items'][0]['record'][index]=value
            with self.assertRaisesRegex(ValueError,'compact item reference'): _unpack_styles(bad)
        for key,value in [('id',item['id']),('source',source)]:
            bad=deepcopy(packed);bad['items'][0][key]=value
            with self.assertRaisesRegex(ValueError,'compact item'): _unpack_styles(bad)
        bad=deepcopy(packed);bad['items'][1]['record'][1]=4
        with self.assertRaisesRegex(ValueError,'Duplicate'): _unpack_styles(bad)
        for index,value in [(0,'not-a-guid'),(1,-1),(2,[0]*129),(2,[True])]:
            bad=deepcopy(packed);bad['placements'][0][index]=value
            with self.assertRaisesRegex(ValueError,'placement'): _unpack_styles(bad)
        bad=deepcopy(packed);bad['source_bases'][0][2]='unknown'
        with self.assertRaisesRegex(ValueError,'source base'): _unpack_styles(bad)
        bad=deepcopy(packed);bad['sheet_id']='a'*513
        with self.assertRaisesRegex(ValueError,'record tables'): _unpack_styles(bad)
        bad=_pack_view_references(_pack_styles(payload));bad['items'][0]['id']='unexpected'
        with self.assertRaisesRegex(ValueError,'Noncanonical'): _pack_item_records(bad)

    def test_shared_styles_preserve_every_value_and_reject_invalid_references(self):
        style=dict(rgba=[0,0,0,1],sources=[dict(stream='/B',start_offset=10,end_offset=20)])
        payload=dict(schema_version=1,items=[dict(id='a',style=style),dict(id='b',style=deepcopy(style)),
                     dict(id='c',style=dict(style,sources=[dict(stream='/B',start_offset=30,end_offset=40)]))])
        packed=_pack_styles(payload)
        self.assertEqual(len(packed['styles']),2)  # Equal appearance, different source evidence stays separate.
        decoded=_unpack_styles(json.loads(sheet_bytes(packed)))
        self.assertEqual(decoded,payload)
        self.assertIs(decoded['items'][0]['style'],decoded['items'][1]['style'])
        self.assertIsNot(decoded['items'][0]['style'],decoded['items'][2]['style'])
        self.assertIs(_unpack_styles(payload),payload)  # Legacy wire v1 remains readable.
        for index in [-1,2,True,'0',.5,None]:
            bad=deepcopy(packed);bad['items'][0]['style_index']=index
            with self.assertRaisesRegex(ValueError,'style table'): _unpack_styles(bad)
        for table in [None,{},[None],[[]],packed['styles']*2]:
            with self.assertRaisesRegex(ValueError,'style table'): _unpack_styles(dict(packed,styles=table))
        bad=deepcopy(packed);bad['items'][0]['style']=style
        with self.assertRaisesRegex(ValueError,'style table'): _unpack_styles(bad)
        with self.assertRaisesRegex(ValueError,'byte limit'): sheet_bytes(packed,10)

    def test_publication_rejects_invalid_style_reference_even_with_correct_hash(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory=Path(tmp)
            scene=build_scene(ROOT/'fixtures/public/SampleBg.idw',directory,Options())
            descriptor=scene['drawing']['sheets'][0]
            payload=json.loads((directory/descriptor['resource']).read_bytes())
            payload['items'][0]['style_index']=len(payload['styles'])
            body=sheet_bytes(payload);digest=hashlib.sha256(body).hexdigest()
            descriptor.update(resource=f'drawing-sheet-{digest}.json',bytes=len(body),sha256=digest)
            (directory/descriptor['resource']).write_bytes(body)
            with self.assertRaisesRegex(ValueError,'style table'):
                validate_drawing_resources(directory,scene)

    def test_view_members_and_export_report_survive_shared_indices(self):
        view = dict(id='v',name='view',item_ids=['b','a'])
        payload = dict(schema_version=1,items=[dict(id='a',style={}),dict(id='b',style={})],
                       views=[view],export_report=dict(sheets=[dict(views=[view])],keep='unchanged'))
        packed = _pack_view_references(_pack_styles(payload))
        self.assertEqual(packed['schema_version'],3)
        self.assertEqual(packed['views'][0]['item_indices'],[1,0])
        self.assertEqual(_unpack_styles(json.loads(sheet_bytes(packed))),payload)
        for export in [False,True]:
            for index in [-1,2,True,'0',None]:
                bad=deepcopy(packed)
                rows=bad['export_report']['sheets'][0]['views'] if export else bad['views']
                rows[0]['item_indices']=[index]
                with self.assertRaisesRegex(ValueError,'view item reference'):
                    _unpack_styles(bad)
            bad=deepcopy(packed)
            rows=bad['export_report']['sheets'][0]['views'] if export else bad['views']
            rows[0]['item_ids']=['a']
            with self.assertRaisesRegex(ValueError,'view item reference'):
                _unpack_styles(bad)
        bad=deepcopy(packed);bad['views'][0]['item_indices']=[0]*100001
        with self.assertRaisesRegex(ValueError,'view item reference'):
            _unpack_styles(bad)

    def test_cli_identifies_renamed_idw_without_optional_geometry_dependencies(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory/'sitecustomize.py').write_text('''import sys
class NoGeometry:
    def find_spec(self, fullname, path=None, target=None):
        if fullname.split('.')[0] in {'ocp_tessellate','cq_acis','cadquery','OCP'}:
            raise ImportError('Optional geometry dependency was requested')
sys.meta_path.insert(0, NoGeometry())
''')
            path = directory/'renamed.ipt'; path.write_bytes((ROOT/'fixtures/public/SampleBg.idw').read_bytes())
            environment = dict(os.environ, PYTHONPATH=str(directory)+os.pathsep+str(ROOT/'python'))
            child = subprocess.Popen([sys.executable, '-m', 'inventor_kit.viewer', str(path), '--no-browser'],
                                     env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                     creationflags=subprocess.CREATE_NEW_PROCESS_GROUP if os.name == 'nt' else 0)
            lines = queue.Queue()
            reader = threading.Thread(target=lambda: lines.put(child.stdout.readline()), daemon=True)
            reader.start()
            try:
                url = lines.get(timeout=15).strip()
                self.assertTrue(url.startswith('http://127.0.0.1:'), url)
                end = time.monotonic() + 15
                while time.monotonic() < end:
                    with urlopen(url+'state.json', timeout=3) as response: result = json.load(response)
                    if result['job_status'] in ('finished','failed'): break
                    time.sleep(.03)
                self.assertEqual(result['job_status'], 'finished')
                self.assertEqual(result['source']['kind'], 'drawing')
                self.assertEqual(result['drawing']['status'], 'experimental_partial')
                self.assertTrue(result['drawing']['sheets'])
                self.assertTrue(result['drawing']['sheets'][0]['resource'])
                self.assertIsNone(result['drawing']['millimeters_per_unit'])
            finally:
                if child.poll() is None:
                    child.send_signal(signal.CTRL_BREAK_EVENT if os.name == 'nt' else signal.SIGINT)
                try:
                    child.communicate(timeout=10)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.communicate(timeout=5)
                    self.fail('Viewer did not stop after its shutdown signal')
                reader.join(timeout=1)
            self.assertEqual(child.returncode, 0)

    def test_scene_schema_and_active_image_allowlist(self):
        with tempfile.TemporaryDirectory() as d:
            directory = Path(d)
            scene = build_scene(ROOT/'fixtures/public/SampleBg.idw', directory, Options(experimental_drawing=True))
            jsonschema.validate(scene, SCHEMA)
            descriptor = scene['drawing']['sheets'][0]
            self.assertNotIn('items', descriptor)
            body = (directory/descriptor['resource']).read_bytes()
            self.assertEqual((len(body), hashlib.sha256(body).hexdigest()), (descriptor['bytes'], descriptor['sha256']))
            payload = json.loads(body)
            # Validate the payload definition with its original local definitions.
            jsonschema.Draft202012Validator({'$defs': SCHEMA['$defs'], '$ref': '#/$defs/sheet_payload'}).validate(payload)
            self.assertEqual(payload['sheet_id'], descriptor['id'])
            self.assertEqual(payload['source_sha256'], scene['source']['sha256'])
            write_scene(directory, scene)
            server, url = create_server(directory)
            thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
            try:
                name = scene['drawing']['images'][0]['resource']
                with urlopen(url+name) as response:
                    self.assertEqual(response.read(), (directory/name).read_bytes())
                with urlopen(url+descriptor['resource']) as response:
                    self.assertEqual(response.read(), body)
                (directory/'drawing-sheet-unregistered.json').write_bytes(b'{}')
                with self.assertRaises(HTTPError): urlopen(url+'drawing-sheet-unregistered.json')
                write_scene(directory, empty_scene('other job'))
                with self.assertRaises(HTTPError) as failure: urlopen(url+name)
                self.assertEqual(failure.exception.code, 404)
                with self.assertRaises(HTTPError): urlopen(url+descriptor['resource'])
            finally:
                server.shutdown(); server.server_close(); thread.join()

    def test_default_and_partial_requests_publish_source_units_without_claiming_mm(self):
        with tempfile.TemporaryDirectory() as temporary:
            for partial in (False, True):
                scene = build_scene(ROOT/'fixtures/public/SampleBg.idw', Path(temporary), Options(allow_partial=partial))
                jsonschema.validate(scene, SCHEMA)
                self.assertEqual(scene['drawing']['status'], 'experimental_partial')
                self.assertTrue(scene['drawing']['sheets'])
                self.assertTrue(all(s['resource'] for s in scene['drawing']['sheets']))
                self.assertTrue(scene['drawing']['images'])
                self.assertEqual(scene['drawing']['units'], 'source_units_unverified')
                self.assertIsNone(scene['drawing']['millimeters_per_unit'])
                self.assertFalse(scene['drawing']['qualified'])

    def test_worker_commits_drawing_only_after_successful_exit(self):
        for target in (None, drawing_failure):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as directory:
                kwargs = dict(target=target) if target else {}
                job = Job(ROOT/'fixtures/public/SampleBg.idw', directory, Options(experimental_drawing=True), **kwargs)
                try:
                    end = time.monotonic() + 15
                    while not job.done and time.monotonic() < end:
                        job.poll(); time.sleep(.02)
                    self.assertTrue(job.done)
                    result = json.loads((Path(directory)/'state.json').read_text())
                    if target:
                        self.assertIsNone(result['drawing'])
                        self.assertEqual(result['job_status'], 'failed')
                    else:
                        self.assertEqual(result['job_status'], 'finished')
                        self.assertEqual(result['drawing']['sheets'][0]['item_count'], 157)
                        jsonschema.validate(result, SCHEMA)
                finally: job.stop()
