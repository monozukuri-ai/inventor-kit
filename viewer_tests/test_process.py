import json
import os
from pathlib import Path
import tempfile
import threading
import time
import unittest
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from inventor_kit.viewer.scene import Options, build_scene, empty_scene, write_scene
from inventor_kit.viewer.server import create_server
from inventor_kit.viewer.worker import Job

ROOT = Path(__file__).resolve().parents[1]


def crash_worker(path, directory, options):
    scene = empty_scene('metadata survives')
    scene['stages']['metadata'] = 'available'
    write_scene(Path(directory), scene)
    write_scene(Path(directory), scene, 'pending.json')
    os._exit(9)


def slow_worker(path, directory, options):
    scene = empty_scene('metadata survives')
    scene['stages']['metadata'] = 'available'
    write_scene(Path(directory), scene)
    time.sleep(30)


def assembly_crash_worker(path, directory, options):
    scene = build_scene(ROOT/'fixtures/public/m5-samplebg/Subassembly.iam', Path(directory), Options())
    scene['job_status'] = 'running'
    scene['nodes'][0]['status'] = 'pending'
    write_scene(Path(directory), scene)
    scene['nodes'][0]['mesh_id'] = 'uncommitted-geometry'
    write_scene(Path(directory), scene, 'pending.json')
    os._exit(9)


class Processes(unittest.TestCase):
    def run_job(self, target=None, timeout=15):
        with tempfile.TemporaryDirectory() as temporary:
            kwargs = {'target':target} if target else {}
            job = Job(ROOT/'fixtures/public/SamplePart.ipt', temporary, Options(timeout=timeout), **kwargs)
            try:
                end = time.monotonic()+20
                while not job.done and time.monotonic() < end:
                    job.poll()
                    time.sleep(.02)
                self.assertTrue(job.done)
                self.assertFalse(job.process.is_alive())
                return json.loads((Path(temporary)/'state.json').read_text())
            finally:
                job.stop()

    def test_spawn_success_commits_geometry_after_exit(self):
        scene = self.run_job()
        self.assertEqual(scene['job_status'], 'finished')
        self.assertTrue(scene['nodes'])

    def test_crash_rejects_pending_result_and_keeps_metadata(self):
        scene = self.run_job(crash_worker)
        self.assertEqual(scene['source']['name'], 'metadata survives')
        self.assertEqual(scene['job_status'], 'failed')
        self.assertEqual(scene['diagnostics'][-1]['code'], 'viewer.worker_failed')
        self.assertFalse(scene['nodes'])

    def test_timeout_terminates_child_and_keeps_metadata(self):
        scene = self.run_job(slow_worker, timeout=2)
        self.assertEqual(scene['source']['name'], 'metadata survives')
        self.assertEqual(scene['diagnostics'][-1]['code'], 'viewer.timeout')
        self.assertEqual(scene['job_status'], 'failed')

    def test_assembly_crash_keeps_occurrence_tree_but_rejects_pending_mesh(self):
        scene = self.run_job(assembly_crash_worker)
        self.assertEqual(scene['job_status'], 'failed')
        self.assertEqual(len(scene['nodes']), 1)
        self.assertEqual(scene['nodes'][0]['occurrence_path'], [1])
        self.assertEqual(scene['nodes'][0]['status'], 'worker_failed')
        self.assertIsNone(scene['nodes'][0]['mesh_id'])
        self.assertFalse(scene['meshes'])

    def test_server_serves_only_session_resources_and_checks_origin(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            write_scene(directory, empty_scene('local file'))
            (directory/'private.txt').write_text('not a scene resource')
            server, url = create_server(directory)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            try:
                with urlopen(url, timeout=3) as response:
                    self.assertIn(b'Inventor Kit', response.read())
                    self.assertIn("connect-src 'self'", response.headers['Content-Security-Policy'])
                with urlopen(url+'state.json', timeout=3) as response:
                    self.assertEqual(json.load(response)['source']['name'], 'local file')
                for suffix, headers, status in [
                    ('private.txt', {}, 404), ('../state.json', {}, 404), ('%2e%2e/state.json', {}, 404),
                    ('state.json', {'Host':'evil.test'}, 403), ('state.json', {'Origin':'https://evil.test'}, 403)]:
                    with self.subTest(suffix=suffix, headers=headers):
                        with self.assertRaises(HTTPError) as error:
                            urlopen(Request(url+suffix, headers=headers), timeout=3)
                        self.assertEqual(error.exception.code, status)
                with self.assertRaises(HTTPError):
                    urlopen(Request(url+'state.json', data=b'write'), timeout=3)
            finally:
                server.shutdown()
                server.server_close()
                thread.join()
