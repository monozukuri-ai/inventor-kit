"""2D scene publication and resource lifetime through the normal worker/server."""
import json
import os
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
SCHEMA = json.loads((ROOT/'schemas/viewer-scene-v1.schema.json').read_text())


def drawing_failure(path, directory, options):
    scene = empty_scene('metadata survives')
    scene['source']['kind'] = 'drawing'
    scene['drawing'] = dict(images=[dict(resource='drawing-image-1.png')], sheets=[])
    (Path(directory)/'drawing-image-1.png').write_bytes(b'pending image')
    write_scene(Path(directory), scene)
    write_scene(Path(directory), scene, 'pending.json')
    os._exit(9)


class DrawingScenes(unittest.TestCase):
    def test_scene_schema_and_active_image_allowlist(self):
        with tempfile.TemporaryDirectory() as d:
            directory = Path(d)
            scene = build_scene(ROOT/'fixtures/public/SampleBg.idw', directory, Options(experimental_drawing=True))
            jsonschema.validate(scene, SCHEMA)
            write_scene(directory, scene)
            server, url = create_server(directory)
            thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
            try:
                name = scene['drawing']['images'][0]['resource']
                with urlopen(url+name) as response:
                    self.assertEqual(response.read(), (directory/name).read_bytes())
                write_scene(directory, empty_scene('other job'))
                with self.assertRaises(HTTPError) as failure: urlopen(url+name)
                self.assertEqual(failure.exception.code, 404)
            finally:
                server.shutdown(); server.server_close(); thread.join()

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
                        self.assertEqual(len(result['drawing']['sheets'][0]['items']), 157)
                        jsonschema.validate(result, SCHEMA)
                finally: job.stop()
