from dataclasses import replace
from decimal import Decimal
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

import cadquery as cq
import jsonschema
import numpy as np
import inventor_kit as ik
from inventor_kit.viewer.scene import Options, build_scene, safe_value
from inventor_kit.viewer.tessellation import build_meshes, validate_mesh

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT/'fixtures/public'
SCHEMA = json.loads((ROOT/'schemas/viewer-scene-v1.schema.json').read_text())


class Scenes(unittest.TestCase):
    def scene(self, name='SamplePart.ipt', options=Options()):
        with tempfile.TemporaryDirectory() as temporary:
            result = build_scene(CORPUS/name, Path(temporary), options)
            jsonschema.validate(result, SCHEMA)
            for resource in [t['resource'] for t in result['thumbnails']] + [b['resource'] for m in result['meshes'] for b in m['buffers'].values()]:
                self.assertTrue((Path(temporary)/resource).exists())
            return result

    def test_public_parts_and_candidate_identity(self):
        for name in ['SamplePart.ipt', 'Cylinder.ipt', 'INV_nist_ftc_09_asme1_2024.ipt']:
            with self.subTest(name=name):
                scene = self.scene(name)
                self.assertEqual(scene['stages']['tessellation'], 'available')
                self.assertEqual(len(scene['nodes']), 1)
                self.assertGreater(scene['meshes'][0]['face_count'], 0)
                self.assertFalse(scene['complete'])
                self.assertEqual(scene['nodes'][0]['source_sha256'], scene['source']['sha256'])
                self.assertEqual(scene['nodes'][0]['candidate_id'], scene['selection']['selected_id'])
        bounds = self.scene()['meshes'][0]['bounds']
        self.assertAlmostEqual(bounds['xmax']-bounds['xmin'], 15, places=5)
        self.assertAlmostEqual(bounds['ymax']-bounds['ymin'], 10, places=5)
        self.assertAlmostEqual(bounds['zmax']-bounds['zmin'], 25, places=5)

    def test_explicit_candidate_wrong_file_and_current_state(self):
        first = self.scene()
        chosen = first['selection']['selected_id']
        self.assertEqual(self.scene(options=Options(candidate_id=chosen))['nodes'], first['nodes'])
        wrong = self.scene('Cylinder.ipt', Options(candidate_id=chosen))
        self.assertFalse(wrong['nodes'])
        self.assertTrue(wrong['diagnostics'])
        refused = self.scene(options=Options(require_current_state=True))
        self.assertFalse(refused['nodes'])
        self.assertTrue(refused['thumbnails'])
        self.assertTrue(refused['candidates'])

    def test_conversion_failure_preserves_document(self):
        scene = self.scene('EPFL_Elytron_140mm_v1.ipt')
        self.assertEqual(scene['stages']['conversion'], 'failed')
        self.assertFalse(scene['nodes'])
        self.assertTrue(scene['properties'])
        self.assertTrue(scene['thumbnails'])
        self.assertTrue(any(d['code'] == 'geometry.curve_unsupported' for d in scene['diagnostics']))

    def test_metadata_only_and_nonpart_document(self):
        self.assertEqual(self.scene(options=Options(metadata_only=True))['stages']['geometry'], 'not_attempted')
        scene = self.scene('BoltedConnection.iam')
        self.assertEqual(scene['source']['kind'], 'assembly')
        self.assertEqual(scene['stages']['conversion'], 'not_attempted')
        self.assertIsNotNone(scene['assembly'])
        self.assertTrue(scene['nodes'])
        self.assertFalse(scene['meshes'])

    def test_resource_limits_and_corrupt_input(self):
        for options in (Options(max_triangles=0), Options(max_buffer_bytes=0)):
            scene = self.scene(options=options)
            self.assertEqual(scene['stages']['tessellation'], 'failed')
            self.assertFalse(scene['meshes'])
            self.assertTrue(scene['thumbnails'])
        scene = self.scene(options=Options(limits=ik.Limits(max_file_bytes=2)))
        self.assertEqual(scene['stages']['metadata'], 'failed')
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)/'broken.ipt'
            path.write_bytes(b'bad')
            scene = build_scene(path, Path(temporary), Options())
            jsonschema.validate(scene, SCHEMA)
            self.assertEqual(scene['stages']['metadata'], 'failed')

    def test_properties_keep_precision_and_do_not_dump_binary(self):
        self.assertEqual(safe_value(2**63+1), '9223372036854775809')
        self.assertEqual(safe_value(Decimal('1234567890.1234567890')), '1234567890.1234567890')
        self.assertEqual(safe_value(ik.FileTime(133000000000000001)), {'ticks_100ns': '133000000000000001'})
        self.assertEqual(safe_value(ik.ClipboardData(1, b'secret')), {'format': '1', 'data': '<6 bytes>'})

    def test_quality_changes_cylinder_mesh(self):
        draft = self.scene('Cylinder.ipt', Options(quality='draft'))
        fine = self.scene('Cylinder.ipt', Options(quality='fine'))
        self.assertGreater(fine['tessellation']['triangle_count'], draft['tessellation']['triangle_count'])
        self.assertLess(fine['tessellation']['linear_deflection_mm'], draft['tessellation']['linear_deflection_mm'])

    def test_mesh_hole_orientation_and_missing_face_rejection(self):
        # An analytic tube is independent of the native parser. Signed mesh
        # volume detects filling its hole or reversing all triangle winding.
        shape = cq.Workplane('XY').circle(10).circle(5).extrude(20).val()
        doc = SimpleNamespace(geometry=SimpleNamespace(source_sha256='a'*64,
            selection=SimpleNamespace(selected_id='synthetic')),
            model=SimpleNamespace(bodies=lambda: [SimpleNamespace(index=1)]))
        with tempfile.TemporaryDirectory() as temporary:
            nodes, meshes, _ = build_meshes([shape], doc, Path(temporary), Options(quality='fine'))
            self.assertEqual(len(nodes), 1)
            mesh = {key: np.fromfile(Path(temporary)/ref['resource'], dtype='<u4' if ref['dtype']=='uint32' else '<f4')
                    for key, ref in meshes[0]['buffers'].items()}
            vertices = mesh['vertices'].astype(np.float64).reshape(-1, 3)
            triangles = mesh['triangles'].reshape(-1, 3)
            a, b, c = (vertices[triangles[:, i]] for i in range(3))
            volume = np.sum(a*np.cross(b,c))/6
            self.assertAlmostEqual(volume/(np.pi*(100-25)*20), 1, delta=0.003)
            normals = mesh['normals'].reshape(-1,3)[triangles[:,0]]
            self.assertTrue(np.all(np.sum(normals*np.cross(b-a,c-a),axis=1) > 0))
            validate_mesh(mesh, shape)
            mesh['triangles_per_face'] = mesh['triangles_per_face'][:-1]
            with self.assertRaisesRegex(ValueError, 'omitted a B-rep face'):
                validate_mesh(mesh, shape)

    def test_option_validation(self):
        for kwargs in ({'quality':'unknown'}, {'timeout':float('nan')}, {'timeout':0},
                       {'max_triangles':-1}, {'metadata_only':True, 'candidate_id':'id'}):
            with self.assertRaises(ValueError):
                Options(**kwargs)
