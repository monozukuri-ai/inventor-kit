"""Native IAM checks and explicitly synthetic graph/placement failure cases."""
from dataclasses import replace
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import cadquery as cq
import inventor_kit as ik
import jsonschema
import numpy as np

from inventor_kit.viewer.assembly import multiply
from inventor_kit.viewer.scene import Options, build_scene
from inventor_kit.viewer.tessellation import IDENTITY, MeshBuilder

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = ROOT/'fixtures/public'
BUNDLE = PUBLIC/'m5-samplebg'
SCHEMA = json.loads((ROOT/'schemas/viewer-scene-v1.schema.json').read_text())


def synthetic_graph():
    """A finite graph with repeated geometry and noncommuting parent/child turns."""
    graph = ik.read_assembly_file(BUNDLE/'Subassembly.iam')
    leaf = graph.instances[0]
    a = ((0., -1., 0., 11.), (1., 0., 0., 2.), (0., 0., 1., 3.), IDENTITY[3])
    b = ((1., 0., 0., 0.), (0., 0., -1., 4.), (0., 1., 0., 0.), IDENTITY[3])
    c = ((1., 0., 0., 35.), (0., 1., 0., 0.), (0., 0., 1., 0.), IDENTITY[3])
    return replace(graph, instances=(
        replace(leaf, name='Nested group', definition=0, local_transform_mm=a, world_transform_mm=a),
        replace(leaf, name='Same display name', parent=0, path=(1, 1), local_transform_mm=b, world_transform_mm=multiply(a, b)),
        replace(leaf, name='Same display name', path=(2,), occurrence_id=2, local_transform_mm=c, world_transform_mm=c)))


class Assemblies(unittest.TestCase):
    def scene(self, path=BUNDLE/'Subassembly.iam', options=Options(allow_unverified_state=True)):
        with tempfile.TemporaryDirectory() as temp:
            scene = build_scene(path, Path(temp), options)
            jsonschema.validate(scene, SCHEMA)
            for mesh in scene['meshes']:
                for ref in mesh['buffers'].values():
                    self.assertEqual((Path(temp)/ref['resource']).stat().st_size, ref['bytes'])
            return scene

    def test_default_requires_state_acknowledgement_and_preserves_inventory(self):
        scene = self.scene(options=Options())
        self.assertEqual(scene['assembly']['structure_status'], 'resolved')
        self.assertEqual(scene['stages']['conversion'], 'not_attempted')
        self.assertEqual(len(scene['nodes']), 1)
        self.assertFalse(scene['meshes'])
        self.assertEqual(scene['diagnostics'][-1]['code'], 'viewer.assembly_state_unverified')
        self.assertTrue(scene['properties'] and scene['thumbnails'])

    def test_native_subassembly_is_placed_and_keeps_source_identity(self):
        scene = self.scene()
        graph = ik.read_assembly_file(BUNDLE/'Subassembly.iam')
        self.assertEqual(scene['stages']['tessellation'], 'available')
        self.assertEqual(scene['assembly']['displayed_instances'], 1)
        self.assertEqual(scene['nodes'][0]['definition_key'], graph.definitions[graph.instances[0].definition].key)
        self.assertEqual(scene['nodes'][0]['world_transform_mm'], [list(r) for r in graph.instances[0].world_transform_mm])
        self.assertEqual(scene['nodes'][0]['occurrence_path'], [1])
        self.assertFalse(scene['complete'])
        self.assertEqual(scene['current_state'], 'unverified')

    def test_native_partial_requires_optin_and_lists_all_omissions(self):
        for allowed in (False, True):
            scene = self.scene(BUNDLE/'SampleBg.iam', Options(allow_unverified_state=True, allow_partial=allowed,
                               search_roots=(str(BUNDLE/'iPartSample'),)))
            self.assertEqual(len(scene['nodes']), 7)
            self.assertEqual({o['reason'] for o in scene['omissions']}, {'geometry_unavailable', 'identity_mismatch'})
            self.assertEqual({tuple(o['path']) for o in scene['omissions']}, {(2,), (4,)})
            self.assertEqual(scene['assembly']['displayed_instances'], 5 if allowed else 0)
            self.assertEqual(len(scene['meshes']), 5 if allowed else 0)
            self.assertEqual(scene['nodes'][1]['local_transform_mm'][0][3], 27.5)  # decoded cm -> mm, once
            self.assertTrue(scene['reference_issues'])
            self.assertIn('Subassembly', scene['nodes'][3]['name'])
            if not allowed:
                self.assertTrue(all(n['mesh_id'] is None for n in scene['nodes']))

    def test_synthetic_repeated_definition_nested_transforms_and_intrinsic_location(self):
        graph = synthetic_graph()
        calls = []
        original = MeshBuilder.add
        def add(builder, shape, mesh_id):
            calls.append(mesh_id)
            return original(builder, shape, mesh_id)
        with patch('inventor_kit.viewer.assembly.read_assembly_file', return_value=graph), patch.object(MeshBuilder, 'add', add):
            with tempfile.TemporaryDirectory() as temp:
                scene = build_scene(BUNDLE/'Subassembly.iam', Path(temp), Options(allow_unverified_state=True))
                jsonschema.validate(scene, SCHEMA)
                self.assertEqual(len(calls), 1)
                self.assertEqual(scene['assembly']['displayed_instances'], 2)
                a, b = scene['nodes'][1:]
                self.assertEqual(a['mesh_id'], b['mesh_id'])
                self.assertNotEqual(a['id'], b['id'])
                self.assertEqual(a['parent'], scene['nodes'][0]['id'])
                mesh = scene['meshes'][0]
                points = np.fromfile(Path(temp)/mesh['buffers']['vertices']['resource'], dtype='<f4').reshape(-1, 3)
                world = np.asarray(a['world_transform_mm'])
                actual = points @ world[:3, :3].T + world[:3, 3]
                # Independently compose CadQuery turns/translations; no viewer
                # matrix helper supplies this expected geometry.
                shape = ik.read_file(BUNDLE/'Triangle.ipt').to_cadquery().val()
                expected = shape.rotate((0, 0, 0), (1, 0, 0), 90).translate((0, 4, 0)).rotate((0, 0, 0), (0, 0, 1), 90).translate((11, 2, 3))
                bb = expected.BoundingBox()
                for i, axis in enumerate('xyz'):
                    self.assertAlmostEqual(actual[:, i].min(), getattr(bb, axis+'min'), places=5)
                    self.assertAlmostEqual(actual[:, i].max(), getattr(bb, axis+'max'), places=5)
        # Shape's own location must not be stripped or replaced by occurrence loc.
        with tempfile.TemporaryDirectory() as temp:
            moved = cq.Workplane('XY').box(2, 4, 6).val().moved(cq.Location((10, 20, 30)))
            mesh = MeshBuilder(Path(temp), Options()).add(moved, 'intrinsic')
            self.assertAlmostEqual(mesh['bounds']['xmin'], 9)
            self.assertAlmostEqual(mesh['bounds']['zmax'], 33)

    def test_unknown_world_placement_is_omitted_without_origin_fallback(self):
        graph = synthetic_graph()
        graph = replace(graph, instances=(*graph.instances[:2], replace(graph.instances[2], world_transform_mm=None)))
        with patch('inventor_kit.viewer.assembly.read_assembly_file', return_value=graph):
            scene = self.scene(options=Options(allow_unverified_state=True, allow_partial=True))
        self.assertEqual(scene['assembly']['displayed_instances'], 1)
        self.assertEqual(scene['nodes'][2]['status'], 'placement_unavailable')
        self.assertIsNone(scene['nodes'][2]['world_transform_mm'])
        self.assertIsNone(scene['nodes'][2]['mesh_id'])

    def test_tessellation_failure_requires_partial_permission(self):
        graph = ik.read_assembly_file(BUNDLE/'SampleBg.iam', search_roots=[BUNDLE/'iPartSample'])
        graph = replace(graph, summary={**graph.summary, 'structure_status':'resolved'}, instances=(graph.instances[0], graph.instances[2]))
        original = MeshBuilder.add
        for allowed in (False, True):
            def add(builder, shape, mesh_id):
                if mesh_id.endswith('-1'):
                    raise ValueError('Controlled missing face')
                return original(builder, shape, mesh_id)
            with patch('inventor_kit.viewer.assembly.read_assembly_file', return_value=graph), patch.object(MeshBuilder, 'add', add):
                scene = self.scene(BUNDLE/'SampleBg.iam', Options(allow_unverified_state=True, allow_partial=allowed))
            self.assertEqual(scene['assembly']['displayed_instances'], 1 if allowed else 0)
            self.assertEqual(scene['omissions'][0]['reason'], 'tessellation_failed')
            self.assertIn('Controlled missing face', scene['omissions'][0]['detail'])

    def test_visibility_suppression_and_substitute_reasons_survive(self):
        original = ik.read_assembly_file(BUNDLE/'Subassembly.iam')
        for field, value, reason in [('suppressed',True,'suppressed'), ('visible',False,'hidden'), ('substitute',True,'unsupported_substitute')]:
            graph = replace(original, instances=(replace(original.instances[0], **{field:value}),))
            with patch('inventor_kit.viewer.assembly.read_assembly_file', return_value=graph):
                scene = self.scene(options=Options(allow_unverified_state=True, allow_partial=True))
            self.assertEqual(scene['nodes'][0]['status'], reason)
            self.assertEqual(scene['omissions'][0]['reason'], reason)
            self.assertFalse(scene['meshes'])

    def test_changed_assembly_or_part_source_aborts_geometry_and_retains_tree(self):
        for name in ('Subassembly.iam', 'Triangle.ipt'):
            with tempfile.TemporaryDirectory() as temp:
                folder = Path(temp)
                for source in ('Subassembly.iam', 'Triangle.ipt'):
                    shutil.copyfile(BUNDLE/source, folder/source)
                graph = ik.read_assembly_file(folder/'Subassembly.iam')
                def resolve(*args, **kwargs):
                    file = folder/name
                    file.write_bytes(file.read_bytes()+b'controlled change')
                    return graph
                with patch('inventor_kit.viewer.assembly.read_assembly_file', side_effect=resolve):
                    scene = self.scene(folder/'Subassembly.iam', Options(allow_unverified_state=True, allow_partial=True))
                self.assertFalse(scene['meshes'])
                self.assertEqual(len(scene['nodes']), 1)
                self.assertTrue(any('changed after assembly resolution' in d['message'] for d in scene['diagnostics']))

    def test_shared_instances_still_consume_rendered_triangle_budget(self):
        graph = synthetic_graph()
        with patch('inventor_kit.viewer.assembly.read_assembly_file', return_value=graph):
            baseline = self.scene()
            limit = baseline['meshes'][0]['triangle_count']
            scene = self.scene(options=Options(allow_unverified_state=True, max_triangles=limit))
        self.assertFalse(scene['meshes'])
        self.assertEqual(scene['stages']['tessellation'], 'failed')
        self.assertIn('occurrence expansion', scene['diagnostics'][-1]['message'])

    def test_holdout_remains_unsupported(self):
        scene = self.scene(PUBLIC/'m5-holdout/wing_assembly.iam', Options())
        self.assertFalse(scene['meshes'])
        self.assertTrue(any('unsupported UFRx schema/section profile' in d['message'] for d in scene['diagnostics']))

    def test_resolver_limits_remain_fail_closed(self):
        graph = ik.read_assembly_file(BUNDLE/'Subassembly.iam', max_documents=1)
        with patch('inventor_kit.viewer.assembly.read_assembly_file', return_value=graph):
            scene = self.scene(options=Options(allow_unverified_state=True, allow_partial=True))
        self.assertEqual(scene['nodes'][0]['status'], 'unavailable')
        self.assertFalse(scene['meshes'])

    def test_reference_failures_are_not_resolved_by_the_viewer(self):
        original = ik.read_assembly_file(BUNDLE/'Subassembly.iam')
        for status in ('missing', 'ambiguous', 'cycle', 'identity_mismatch'):
            graph = replace(original, summary={**original.summary, 'structure_status':'partial'},
                            instances=(replace(original.instances[0], resolution=status, definition=None),))
            with patch('inventor_kit.viewer.assembly.read_assembly_file', return_value=graph):
                scene = self.scene(options=Options(allow_unverified_state=True, allow_partial=True))
            self.assertEqual(scene['nodes'][0]['status'], status)
            self.assertEqual(scene['omissions'][0]['reason'], status)
            self.assertFalse(scene['meshes'])

    def test_alias_change_during_tessellation_rejects_pending_geometry(self):
        graph = ik.read_assembly_file(BUNDLE/'Subassembly.iam')
        original = MeshBuilder.add
        with tempfile.TemporaryDirectory() as temp:
            alias = Path(temp)/'alias.ipt'
            shutil.copyfile(BUNDLE/'Triangle.ipt', alias)
            part = replace(graph.definitions[1], source_paths=(*graph.definitions[1].source_paths, str(alias)))
            graph = replace(graph, definitions=(graph.definitions[0], part))
            def add(builder, shape, mesh_id):
                result = original(builder, shape, mesh_id)
                alias.write_bytes(alias.read_bytes()+b'controlled change during meshing')
                return result
            with patch('inventor_kit.viewer.assembly.read_assembly_file', return_value=graph), patch.object(MeshBuilder, 'add', add):
                scene = self.scene()
            self.assertFalse(scene['meshes'])
            self.assertTrue(scene['nodes'])
            self.assertIn('changed after assembly resolution', scene['diagnostics'][-1]['message'])
