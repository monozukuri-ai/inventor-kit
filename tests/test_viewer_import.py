import subprocess
import sys
import unittest


class ViewerImport(unittest.TestCase):
    def test_metadata_path_does_not_import_optional_viewer_geometry(self):
        code = '''
import sys
class BlockGeometry:
    def find_spec(self, fullname, path=None, target=None):
        if fullname.split('.')[0] in {'cadquery', 'cq_acis', 'ocp_tessellate', 'OCP'}:
            raise AssertionError('Unexpected geometry import: ' + fullname)
sys.meta_path.insert(0, BlockGeometry())
import inventor_kit
import inventor_kit.viewer
from inventor_kit.viewer.scene import build_scene, Options
from tempfile import TemporaryDirectory
from pathlib import Path
with TemporaryDirectory() as d:
    s = build_scene(Path('fixtures/public/SamplePart.ipt'), Path(d), Options(metadata_only=True))
    assert s['stages']['metadata'] == 'available'
    assert s['properties'] and s['thumbnails']
'''
        subprocess.run([sys.executable, '-c', code], check=True)

    def test_cli_help_and_invalid_options(self):
        help_ = subprocess.run([sys.executable, '-m', 'inventor_kit.viewer', '--help'], capture_output=True, text=True)
        self.assertEqual(help_.returncode, 0)
        self.assertIn('--candidate-id', help_.stdout)
        for args in (['--metadata-only','--candidate-id','bad'], ['--port','-1'], ['--timeout','nan']):
            result = subprocess.run([sys.executable, '-m', 'inventor_kit.viewer', 'part.ipt', *args], capture_output=True)
            self.assertEqual(result.returncode, 2)
