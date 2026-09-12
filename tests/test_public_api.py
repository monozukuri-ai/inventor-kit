"""Public Python/native integration. Corpus-dependent checks skip when absent."""
from pathlib import Path
import hashlib
import json
import math
import subprocess
import sys
import tempfile
import unittest

import inventor_kit
from cq_acis import CadQueryConversionError, RawEntity, TorusSurfaceEntity

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = {r['file']: r for r in json.loads((ROOT / 'fixtures/manifest.json').read_text())}


def fixture(case, name):
    path = ROOT / 'fixtures/public' / name
    if not path.exists():
        case.skipTest('Run scripts/fetch_public_samples.py for public fixtures')
    data = path.read_bytes()
    case.assertEqual(hashlib.sha256(data).hexdigest(), MANIFEST[name]['sha256'])
    return path, data


class PublicAPI(unittest.TestCase):
    def test_invalid_bytes_raise(self):
        with self.assertRaisesRegex(ValueError, 'CFB'):
            inventor_kit.read(b'not an Inventor file')
        with self.assertRaises(ValueError):
            inventor_kit.read(b'', source_id='')
        with self.assertRaises(TypeError):
            inventor_kit.read('not bytes')

    def test_public_zstd_parts_form_valid_analytic_solids(self):
        for name, volume, area, faces in [
            ('Cylinder.ipt', math.pi * 12.5**2 * 10, 2 * math.pi * 12.5 * (12.5 + 10), 3),
            ('SamplePart.ipt', 15 * 25 * 10, 2 * (15*25 + 15*10 + 25*10), 6),
        ]:
            with self.subTest(name=name):
                path, data = fixture(self, name)
                doc = inventor_kit.read(data, source_id=name)
                self.assertEqual(doc.summary['status'], 'decoded_subset')
                self.assertEqual(doc.summary['carrier']['codec'], 'zstd')
                self.assertEqual(doc.model.metadata.units_mm, 10.0)
                shape = doc.to_cadquery().val()
                self.assertTrue(shape.isValid())
                self.assertEqual(len(shape.Solids()), 1)
                self.assertEqual(len(shape.Faces()), faces)
                self.assertAlmostEqual(shape.Volume(), volume, places=7)
                self.assertAlmostEqual(shape.Area(), area, places=7)
                # Dimensional regressions are analytic checks, not a vendor oracle.
                carrier = doc.summary['carrier']
                self.assertTrue(carrier['has_history'])
                for entity in doc.model.entities:
                    raw = entity if isinstance(entity, RawEntity) else entity.raw
                    span = raw.source
                    self.assertEqual(span.source_id, carrier['source_id'])
                    self.assertEqual(raw.raw_data, doc.kernel_bytes[
                        span.start_offset-carrier['kernel_offset']:span.end_offset-carrier['kernel_offset']])
                    self.assertIsNone(raw.record)
                self.assertEqual(inventor_kit.read_file(path).kernel_bytes, doc.kernel_bytes)

    def test_nist_torus_profile_converts_and_keeps_history_opaque(self):
        _, data = fixture(self, 'INV_nist_ftc_11_asme1_2021.ipt')
        doc = inventor_kit.read(data)
        self.assertEqual(doc.summary['carrier']['codec'], 'zlib')
        self.assertEqual(doc.summary['carrier']['save_version'], 22600)
        self.assertEqual(sum(isinstance(e, TorusSurfaceEntity) for e in doc.model.entities), 4)
        self.assertTrue(any(d.code == 'sab.history_opaque' for d in doc.model.diagnostics))
        shape = doc.to_cadquery().val()
        self.assertTrue(shape.isValid())
        self.assertGreater(shape.Volume(), 0)

    def test_nist_unqualified_spline_is_not_silently_approximated(self):
        _, data = fixture(self, 'INV_nist_ftc_07_asme1_2024.ipt')
        doc = inventor_kit.read(data)
        self.assertTrue(any(isinstance(e, RawEntity) and e.type_name == 'spline-surface' for e in doc.model.entities))
        with self.assertRaises(CadQueryConversionError) as failure:
            doc.to_cadquery()
        self.assertEqual(failure.exception.code, 'geometry.surface_unsupported')

    def test_explicit_asm_nurbs_keep_native_spans_and_vertex_checks(self):
        from cq_acis import BSplineCurveEntity, BSplineSurfaceEntity, CadQueryConverter, CoedgeEntity, EdgeEntity
        _, data = fixture(self, 'INV_nist_ftc_07_asme1_2021.ipt')
        doc = inventor_kit.read(data)
        curves = [e for e in doc.model.entities if isinstance(e, BSplineCurveEntity)]
        surfaces = [e for e in doc.model.entities if isinstance(e, BSplineSurfaceEntity)]
        self.assertEqual((len(curves), len(surfaces)), (64, 18))
        finite = [e for e in surfaces if e.index in (1108, 1626)]
        self.assertEqual(len(finite), 2)
        for surface in finite:
            self.assertIsNotNone(surface.u_range.lower)
            self.assertIsNotNone(surface.v_range.upper)
        carrier = doc.summary['carrier']
        for entity in [*curves, *surfaces]:
            entity.validate()
            span = entity.raw.source
            self.assertEqual(span.source_id, carrier['source_id'])
            self.assertEqual(entity.raw.raw_data, doc.kernel_bytes[
                span.start_offset-carrier['kernel_offset']:span.end_offset-carrier['kernel_offset']])
        converter = CadQueryConverter(doc.model)
        placement = converter._body_placement(doc.model.bodies()[0])
        passed = 0
        for coedge in doc.model.entities:
            if not isinstance(coedge, CoedgeEntity):
                continue
            edge = doc.model.resolve(coedge.edge)
            if not isinstance(edge, EdgeEntity) or not isinstance(doc.model.resolve(edge.curve), BSplineCurveEntity):
                continue
            result = converter._edge(coedge, placement)
            self.assertTrue(result.isValid())
            passed += 1
        self.assertEqual(passed, 88)
        self.assertEqual(len(converter.tolerant_endpoints), 112)
        self.assertTrue(all(e['deviation_mm'] <= e['tolerance_mm'] for e in converter.tolerant_endpoints))
        with self.assertRaises(CadQueryConversionError):
            doc.to_cadquery()

    def test_sphere_holes_export_a_valid_solid(self):
        _, data = fixture(self, 'Demo-Status-0.4.ipt')
        doc = inventor_kit.read(data)
        shape = doc.to_cadquery().val()
        self.assertTrue(shape.isValid())
        self.assertEqual(len(shape.Solids()), 1)
        self.assertGreater(shape.Volume(), 0)

    def test_assembly_is_inventory_only(self):
        _, data = fixture(self, 'BoltedConnection.iam')
        doc = inventor_kit.read(data)
        self.assertIsNone(doc.model)
        self.assertEqual(doc.summary['kind'], 'assembly')
        self.assertTrue(doc.summary['streams'])
        with self.assertRaisesRegex(ValueError, 'geometry unavailable'):
            doc.to_cadquery()

    def test_empty_stored_table_does_not_export_an_empty_workplane(self):
        _, data = fixture(self, 'Demo-Status-0.1.ipt')
        doc = inventor_kit.read(data)
        self.assertIsNotNone(doc.model)
        with self.assertRaisesRegex(ValueError, 'no decoded bodies'):
            doc.to_cadquery()

    def test_cli_exports_step_and_round_trip_preserves_geometry(self):
        import cadquery as cq
        path, _ = fixture(self, 'Cylinder.ipt')
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'cylinder.step'
            result = subprocess.run([sys.executable, '-m', 'inventor_kit', str(path), '--step', str(output)], capture_output=True, text=True, check=True)
            self.assertEqual(json.loads(result.stdout)['status'], 'decoded_subset')
            shape = cq.importers.importStep(str(output)).val()
            self.assertTrue(shape.isValid())
            self.assertAlmostEqual(shape.Volume(), math.pi * 12.5**2 * 10, places=6)
            self.assertEqual(len(shape.Faces()), 3)
