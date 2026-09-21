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
from urllib.error import HTTPError
from urllib.request import urlopen

import jsonschema
from inventor_kit.viewer.scene import Options, build_scene, empty_scene, write_scene
from inventor_kit.viewer.server import create_server
from inventor_kit.viewer.worker import Job

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
                self.assertEqual(result['drawing']['status'], 'unavailable')
                self.assertTrue(result['drawing']['sheets'])
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

    def test_strict_and_partial_requests_preserve_reasons_without_publishing_unknown_units(self):
        with tempfile.TemporaryDirectory() as temporary:
            for partial in (False, True):
                scene = build_scene(ROOT/'fixtures/public/SampleBg.idw', Path(temporary), Options(allow_partial=partial))
                jsonschema.validate(scene, SCHEMA)
                self.assertEqual(scene['drawing']['status'], 'unavailable')
                self.assertTrue(scene['drawing']['sheets'])
                self.assertTrue(all(s['resource'] is None for s in scene['drawing']['sheets']))
                self.assertFalse(scene['drawing']['images'])
                self.assertTrue(any(d['code'] == 'drawing.units_unverified' for d in scene['diagnostics']))
                self.assertFalse(list(Path(temporary).glob('drawing-*')))

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
