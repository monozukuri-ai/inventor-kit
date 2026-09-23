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
    s = build_scene(Path('fixtures/public/m5-samplebg/Subassembly.iam'), Path(d), Options())
    assert s['assembly']['structure_status'] == 'resolved'
    assert len(s['nodes']) == 1 and not s['meshes']
    assert s['stages']['conversion'] == 'not_attempted'
'''
        subprocess.run([sys.executable, '-c', code], check=True)

    def test_cli_help_and_invalid_options(self):
        help_ = subprocess.run([sys.executable, '-m', 'inventor_kit.viewer', '--help'], capture_output=True, text=True)
        self.assertEqual(help_.returncode, 0)
        self.assertIn('--candidate-id', help_.stdout)
        self.assertIn('IDW drawings open in saved source coordinates by default', help_.stdout)
        self.assertNotIn('--experimental-drawing', help_.stdout)
        for args in (['--metadata-only','--candidate-id','bad'], ['--port','-1'], ['--timeout','nan'],
                     ['--metadata-only', '--allow-unverified-state']):
            result = subprocess.run([sys.executable, '-m', 'inventor_kit.viewer', 'part.ipt', *args], capture_output=True)
            self.assertEqual(result.returncode, 2)

    def test_document_options_follow_identified_kind(self):
        from pathlib import Path
        from tempfile import TemporaryDirectory
        from inventor_kit.viewer.scene import build_scene, Options
        fixtures = Path(__file__).resolve().parents[1]/'fixtures/public'
        cases = [('SamplePart.ipt', 'renamed.idw', Options(search_roots=('parts',)), 'IAM assemblies only'),
                 ('m5-samplebg/Subassembly.iam', 'renamed.ipt', Options(candidate_id='bad'), 'IPT parts only'),
                 ('m5-samplebg/Subassembly.iam', 'renamed.idw', Options(allow_partial=True), 'requires --allow-unverified-state'),
                 ('SampleBg.idw', 'renamed.iam', Options(candidate_id='bad'), 'IDW'),
                 ('SamplePart.ipt', 'renamed.idw', Options(experimental_drawing=True), 'identified IDW')]
        for source, renamed, options, reason in cases:
            with self.subTest(source=source, options=options), TemporaryDirectory() as temporary:
                directory = Path(temporary)
                path = directory/renamed
                path.write_bytes((fixtures/source).read_bytes())
                scene = build_scene(path, directory, options)
                self.assertEqual(scene['job_status'], 'finished')
                self.assertEqual(scene['stages']['geometry'], 'failed')
                self.assertEqual(scene['stages']['metadata'], 'available')
                self.assertTrue(any(reason in d['message'] for d in scene['diagnostics']), scene['diagnostics'])
