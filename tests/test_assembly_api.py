"""M5 public native fixtures and explicitly synthetic relocation/mutation checks."""
from dataclasses import replace
import hashlib
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import uuid

import inventor_kit as ik
import olefile

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = ROOT / 'fixtures/public'
FIXTURES = PUBLIC / 'm5-samplebg'
MANIFEST = json.loads((ROOT / 'fixtures/assembly-manifest.json').read_text())


def relink(path, reference_id, document_id):
    """Controlled test mutation; this never repairs or overwrites upstream fixtures."""
    doc = ik.inspect_assembly_file(path)
    ref = next(r for r in doc.references if r['reference_id'] == reference_id)
    old, new = uuid.UUID(ref['document_id']).bytes_le, uuid.UUID(document_id).bytes_le
    data = io.BytesIO(path.read_bytes())
    with olefile.OleFileIO(data, write_mode=True) as ole:
        stream = bytearray(ole.openstream('UFRxDoc').read())
        start, end = ref['source']['start_offset'], ref['source']['end_offset']
        record = stream[start:end]
        if record.count(old) != 1:
            raise AssertionError('reference identity is not unique in its bounded record')
        stream[start:end] = record.replace(old, new)
        ole.write_stream('UFRxDoc', bytes(stream))
        output = data.getvalue()
    path.write_bytes(output)


class AssemblyAPI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        for row in MANIFEST:
            path = PUBLIC / row['file']
            if not path.exists():
                raise unittest.SkipTest('Run scripts/fetch_assembly_samples.py')
            if hashlib.sha256(path.read_bytes()).hexdigest() != row['sha256']:
                raise AssertionError(f'Fixture SHA-256 mismatch: {path}')

    def test_native_saved_ids_placements_and_provenance(self):
        doc = ik.inspect_assembly_file(FIXTURES / 'SampleBg.iam')
        self.assertEqual(len(doc.references), 8)
        self.assertEqual(len(doc.occurrences), 7)
        self.assertEqual([o.occurrence_id for o in doc.occurrences], list(range(1, 8)))
        self.assertTrue(all(o.status == 'resolved' for o in doc.occurrences))
        self.assertEqual([row[3] for row in doc.occurrences[1].local_transform_mm[:3]], [27.5, 20., 40.])
        self.assertIsNone(doc.summary['ufrx']['active_model_state'])
        self.assertEqual(doc.summary['current_state'], 'unverified')
        for o in doc.occurrences:
            self.assertIsNone(o.visible)
            self.assertIsNone(o.substitute)
            self.assertEqual(o.source['byte_domain'], 'cfb_stream')
            self.assertEqual(o.placement['evidence']['source']['byte_domain'], 'inflated_stream')
            self.assertEqual(o.placement['evidence']['compressed_source']['start_offset'], 18)
        members = [r for r in doc.references if r['member_name']]
        self.assertEqual([r['member_factory_reference'] for r in members], [6, 6, 6])
        # The section and tag are retained without declaring a name/visibility
        # meaning. Independently check their bytes inside the bounded UFRx stream.
        with olefile.OleFileIO(FIXTURES / 'SampleBg.iam') as ole:
            stream = ole.openstream('UFRxDoc').read()
        self.assertEqual(len(doc.stored_occurrences), 7)
        properties = [p for o in doc.stored_occurrences for p in o['properties']]
        self.assertTrue(properties)
        self.assertTrue(any(p['value']['type'] == 'string' for p in properties))
        for prop in properties:
            span = prop['source']
            data = stream[span['start_offset']:span['end_offset']]
            self.assertEqual((data[1], data[6]), (prop['tag'], prop['tag']))
            if prop['value']['type'] == 'string':
                size = int.from_bytes(data[7:11], 'little')
                self.assertEqual(data[11:11 + 2 * size].decode('utf-16-le'), prop['value']['value'])

    def test_real_bundle_identity_mismatch_is_not_silently_relinked(self):
        graph = ik.read_assembly_file(FIXTURES / 'SampleBg.iam', search_roots=[FIXTURES / 'iPartSample'])
        self.assertEqual(graph.structure_status, 'partial')
        self.assertFalse(graph.complete)
        self.assertEqual(len(graph.instances), 7)
        self.assertEqual(graph.instances[3].resolution, 'identity_mismatch')
        self.assertIsNone(graph.instances[3].definition)
        reference = graph.definitions[0].references[3]
        self.assertIn('4e07fdab-970f-47dd-89f9-d9106da51b8b', reference['detail'])
        self.assertIn('baf3d458-acf5-4b68-ab59-2943c6e023b6', reference['detail'])
        self.assertEqual(len(graph.definitions), 9)
        self.assertFalse(any(Path(d.path).name == 'Triangle.ipt' for d in graph.definitions))

    def test_native_subassembly_solid_and_strict_conversion(self):
        graph = ik.read_assembly_file(FIXTURES / 'Subassembly.iam')
        self.assertEqual(graph.structure_status, 'resolved')
        self.assertEqual((len(graph.definitions), len(graph.instances)), (2, 1))
        with self.assertRaisesRegex(ValueError, 'Model State'):
            graph.to_cadquery()
        result = graph.to_cadquery(allow_unverified_state=True)
        self.assertEqual((result.converted_instances, result.converted_definitions), (1, 1))
        self.assertEqual(result.omissions, ())
        self.assertFalse(result.complete)
        self.assertTrue(result.assembly.toCompound().isValid())
        self.assertEqual(len(result.assembly.toCompound().Solids()), 1)

    def test_partial_conversion_reports_every_missing_part(self):
        graph = ik.read_assembly_file(FIXTURES / 'SampleBg.iam', search_roots=[FIXTURES / 'iPartSample'])
        with self.assertRaises(ik.AssemblyConversionError) as error:
            graph.to_cadquery(allow_unverified_state=True)
        result = error.exception.result
        self.assertEqual(result.converted_instances, 5)
        self.assertEqual({o.path for o in result.omissions}, {(2,), (4,)})
        self.assertEqual({o.reason for o in result.omissions}, {'geometry_unavailable', 'identity_mismatch'})
        self.assertTrue(result.reference_issues)
        self.assertTrue(result.diagnostics)

    def test_synthetic_relink_repeated_geometry_and_nested_world_bbox(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary) / 'assembly'
            shutil.copytree(FIXTURES, package)
            root = package / 'SampleBg.iam'
            child_id = ik.inspect_assembly_file(package / 'Subassembly.iam').summary['ufrx']['document_id']
            part_id = ik.inspect_assembly_file(package / 'SamplePart.ipt').summary['ufrx']['document_id']
            relink(root, 4, child_id)
            relink(root, 2, part_id)
            shutil.copyfile(package / 'SamplePart.ipt', package / 'SampleComplexPart.ipt')
            graph = ik.read_assembly_file(root, search_roots=[package / 'iPartSample'])
            self.assertEqual(len(graph.instances), 8)
            self.assertEqual(graph.instances[0].definition, graph.instances[1].definition)
            nested = next(i for i in graph.instances if i.path == (4, 1))
            parent = next(i for i in graph.instances if i.path == (4,))
            self.assertEqual(nested.world_transform_mm, parent.world_transform_mm)
            result = graph.to_cadquery(allow_partial=True, allow_unverified_state=True)
            self.assertEqual((result.converted_instances, result.converted_definitions), (7, 6))
            self.assertFalse(result.omissions)
            compound = result.assembly.toCompound()
            self.assertEqual(len(compound.Solids()), 7)
            self.assertTrue(compound.isValid())
            self.assertTrue(any('/Triangle ' in name for name in result.assembly.objects))
            # Independent CadQuery translation of the seven source solids. This
            # is an analytic composition check, explicitly not a vendor oracle.
            import cadquery as cq
            expected = []
            for instance in graph.instances:
                if instance.definition is None:
                    continue
                definition = graph.definitions[instance.definition]
                if definition.document.summary['kind'] != 'part':
                    continue
                matrix = instance.world_transform_mm
                shape = ik.read_file(definition.path).to_cadquery().val()
                expected.append(shape.translate(tuple(row[3] for row in matrix[:3])))
            expected = cq.Compound.makeCompound(expected)
            actual_box, expected_box = compound.BoundingBox(), expected.BoundingBox()
            for field in ('xmin', 'ymin', 'zmin', 'xmax', 'ymax', 'zmax'):
                self.assertAlmostEqual(getattr(actual_box, field), getattr(expected_box, field), places=8)
            self.assertAlmostEqual(compound.Volume(), expected.Volume(), places=6)
            alias = package / 'SampleComplexPart.ipt'
            alias.write_bytes(alias.read_bytes() + b'changed alias')
            result = graph.to_cadquery(allow_partial=True, allow_unverified_state=True)
            self.assertEqual(result.converted_instances, 5)
            self.assertTrue(all('source alias changed' in o.detail for o in result.omissions))

    def test_changed_part_fingerprint_and_nonrigid_transform_fail_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            package = Path(temporary)
            for name in ('Subassembly.iam', 'Triangle.ipt'):
                shutil.copyfile(FIXTURES / name, package / name)
            graph = ik.read_assembly_file(package / 'Subassembly.iam')
            part = package / 'Triangle.ipt'
            part.write_bytes(part.read_bytes() + b'changed')
            with self.assertRaises(ik.AssemblyConversionError) as error:
                graph.to_cadquery(allow_unverified_state=True)
            self.assertIn('changed after assembly resolution', error.exception.result.omissions[0].detail)
        graph = ik.read_assembly_file(FIXTURES / 'Subassembly.iam')
        instance = graph.instances[0]
        for x in (2.0, -1.0):
            matrix = ((x, 0., 0., 0.), (0., 1., 0., 0.), (0., 0., 1., 0.), (0., 0., 0., 1.))
            changed = replace(graph, instances=(replace(instance, local_transform_mm=matrix),))
            with self.assertRaises(ik.AssemblyConversionError):
                changed.to_cadquery(allow_unverified_state=True)

    def test_rotation_direction_and_distinct_visibility_states(self):
        graph = ik.read_assembly_file(FIXTURES / 'Subassembly.iam')
        matrix = ((0., -1., 0., 10.), (1., 0., 0., 20.), (0., 0., 1., 30.), (0., 0., 0., 1.))
        changed = replace(graph, instances=(replace(graph.instances[0], local_transform_mm=matrix),))
        actual = changed.to_cadquery(allow_unverified_state=True).assembly.toCompound()
        expected = (ik.read_file(FIXTURES / 'Triangle.ipt').to_cadquery().val()
                    .rotate((0, 0, 0), (0, 0, 1), 90).translate((10, 20, 30)))
        for field in ('xmin', 'ymin', 'zmin', 'xmax', 'ymax', 'zmax'):
            self.assertAlmostEqual(getattr(actual.BoundingBox(), field), getattr(expected.BoundingBox(), field), places=8)
        for field, value, reason in [('suppressed', True, 'suppressed'), ('visible', False, 'hidden'),
                                     ('substitute', True, 'unsupported_substitute')]:
            changed = replace(graph, instances=(replace(graph.instances[0], **{field: value}),))
            result = changed.to_cadquery(allow_partial=True, allow_unverified_state=True)
            self.assertEqual(result.omissions[0].reason, reason)
            self.assertEqual(result.converted_instances, 0)

    def test_limits_and_missing_reference_preserve_result(self):
        graph = ik.read_assembly_file(FIXTURES / 'Subassembly.iam', max_documents=1)
        self.assertEqual(graph.structure_status, 'partial')
        self.assertEqual(len(graph.instances), 1)
        self.assertEqual(graph.instances[0].resolution, 'unavailable')
        graph = ik.FileSystemResolver(max_instances=0).read(FIXTURES / 'Subassembly.iam')
        self.assertTrue(graph.summary['diagnostics'])
        self.assertFalse(graph.complete)

    def test_holdout_is_profile_gated_without_tuning(self):
        doc = ik.inspect_assembly_file(PUBLIC / 'm5-holdout/wing_assembly.iam')
        self.assertEqual(doc.status, 'unavailable')
        self.assertTrue(any('unsupported UFRx schema/section profile' in d['message'] for d in doc.diagnostics))
        self.assertFalse(doc.occurrences)

    def test_metadata_and_resolver_do_not_import_geometry(self):
        code = '''import sys
class DenyGeometry:
    def find_spec(self, fullname, path=None, target=None):
        if fullname.split('.')[0] in ('cq_acis', 'cadquery', 'OCP'):
            raise AssertionError('geometry import: ' + fullname)
sys.meta_path.insert(0, DenyGeometry())
import inventor_kit as ik
doc = ik.inspect_assembly_file(sys.argv[1])
graph = ik.read_assembly_file(sys.argv[1])
assert len(doc.occurrences) == len(graph.instances) == 1
assert 'cq_acis' not in sys.modules
'''
        subprocess.run([sys.executable, '-c', code, str(FIXTURES / 'Subassembly.iam')], check=True)


if __name__ == '__main__':
    unittest.main()
