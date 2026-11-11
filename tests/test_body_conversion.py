"""Real mixed-body files, complete provenance, and checked selection transport."""
from dataclasses import replace
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import inventor_kit as ik
import jsonschema
from cq_acis import CadQueryConversionError

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / 'fixtures/public'


class Bodies(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.documents = [ik.read_file(CORPUS / f'INV_nist_ftc_06_asme1_{year}.ipt') for year in (2021, 2024)]
        cls.results = [doc.convert_bodies() for doc in cls.documents]

    def test_every_body_is_preserved_with_bound_source_ids(self):
        for doc, result in zip(self.documents, self.results):
            with self.subTest(candidate=result.candidate_id):
                self.assertEqual([b.body_index for b in result.bodies], [1, 2, 3])
                self.assertEqual([b.status for b in result.bodies], ['converted_solid', 'unsupported', 'unsupported'])
                self.assertEqual(result.bodies[0].metrics['faces'], 146)
                self.assertFalse(result.geometry_complete)
                self.assertFalse(result.complete)
                self.assertEqual(result.bodies[1].diagnostics[0].code, 'geometry.sewing_no_shell')
                for body in result.bodies:
                    source = body.source
                    offset = doc.summary['carrier']['kernel_offset']
                    raw = doc.model.resolve(body.body_index).raw
                    self.assertEqual(raw.raw_data, doc.kernel_bytes[source['start_offset']-offset:source['end_offset']-offset])
                with self.assertRaises(CadQueryConversionError):
                    doc.to_cadquery()
        self.assertNotEqual(self.results[0].bodies[0].id, self.results[1].bodies[0].id)

    def test_selection_requires_optin_and_rejects_foreign_empty_duplicate_ids(self):
        result = self.results[0]
        for ids in ([], [result.bodies[0].id] * 2, [self.results[1].bodies[0].id], result.bodies[0].id):
            with self.subTest(ids=ids), self.assertRaises(ValueError):
                result.to_cadquery(body_ids=ids, allow_partial=True)
        with self.assertRaises(ik.BodyConversionError) as caught:
            result.to_cadquery(body_ids=[result.bodies[0].id])
        self.assertIs(caught.exception.result, result)
        with self.assertRaises(ik.BodyConversionError):
            result.to_cadquery(body_ids=[result.bodies[1].id], allow_partial=True)
        report = result.report(body_ids=[result.bodies[0].id])
        jsonschema.validate(report, json.loads((ROOT/'schemas/conversion-report-v1.schema.json').read_text()))
        self.assertTrue(report['selection_complete'])
        self.assertFalse(report['geometry_complete'])
        self.assertEqual([o['reason'] for o in report['omissions']], ['not_selected', 'not_selected'])
        json.dumps(report, allow_nan=False)

    def test_selected_solid_step_roundtrip_and_atomic_sidecar(self):
        for result in self.results:
            with tempfile.TemporaryDirectory() as temporary:
                path = Path(temporary) / 'selected.step'
                with self.assertRaises(ik.BodyConversionError):
                    result.export_step(path)
                self.assertFalse(path.exists())
                report = result.export_step(path, body_ids=[result.bodies[0].id], allow_partial=True)
                self.assertEqual(report['roundtrip']['status'], 'passed')
                self.assertEqual(report['actual'][1]['geometry']['faces'], 146)
                self.assertEqual(report['actual'][1]['geometry']['solids'], 1)
                self.assertEqual(len(report['omissions']), 2)
                self.assertTrue(report['selection_complete'])
                self.assertFalse(report['geometry_complete'])
                self.assertFalse(report['current_state_verified'])
                self.assertEqual(report['step_sha256'], hashlib.sha256(path.read_bytes()).hexdigest())
                saved = json.loads(path.with_suffix('.step.json').read_text())
                self.assertEqual(saved['conversion']['candidate_id'], result.candidate_id)
                with self.assertRaises(FileExistsError):
                    result.export_step(path, allow_partial=True)
                bad = Path(temporary) / 'failed.step'
                with patch('inventor_kit.assembly_step._read_xde', return_value=[]):
                    with self.assertRaisesRegex(ValueError, 'round-trip'):
                        result.export_step(bad, allow_partial=True)
                self.assertFalse(bad.exists())
                self.assertFalse(bad.with_suffix('.step.json').exists())

    def test_complete_part_identity_and_inspection_contract(self):
        path = CORPUS / 'SamplePart.ipt'
        result = ik.read_file(path).convert_bodies()
        self.assertTrue(result.geometry_complete)
        self.assertEqual(result.to_cadquery().val().Volume(), 3750)
        self.assertEqual(result.report()['status'], 'success')
        copy = ik.read(path.read_bytes(), source_id='same-bytes-other-path').convert_bodies()
        self.assertEqual(result.bodies[0].id, copy.bodies[0].id)
        with self.assertRaisesRegex(ValueError, 'not requested'):
            ik.inspect_file(path).convert_bodies()
        unavailable = ik.read_file(path, require_current_state=True).convert_bodies()
        self.assertEqual(unavailable.report()['status'], 'unsupported')
        with self.assertRaises(ik.BodyConversionError):
            unavailable.to_cadquery(allow_partial=True)

    def test_unexpected_native_errors_are_not_silently_omitted(self):
        with patch('cq_acis.CadQueryConverter.convert_body', side_effect=RuntimeError('native failure')):
            with self.assertRaisesRegex(RuntimeError, 'native failure'):
                self.documents[0].convert_bodies()

    def test_undecoded_body_cannot_disappear_from_inventory(self):
        document = self.documents[0]
        entities = tuple(e.raw if e.index == 2 else e for e in document.model.entities)
        partial = replace(document, model=replace(document.model, entities=entities))
        result = partial.convert_bodies()
        self.assertEqual([b.body_index for b in result.bodies], [1, 2, 3])
        self.assertEqual(result.bodies[1].status, 'unsupported')
        self.assertEqual(result.bodies[1].diagnostics[0].code, 'geometry.body_unsupported')
        self.assertEqual(result.bodies[1].id, self.results[0].bodies[1].id)
        self.assertEqual(len(result.report()['omissions']), 2)
        self.assertFalse(result.geometry_complete)


class Backends(unittest.TestCase):
    """The OCCT backend gives the same bodies and metrics without CadQuery shapes."""

    def test_occt_backend_matches_cadquery_and_returns_topods_shapes(self):
        from OCP.TopoDS import TopoDS_Shape
        import cadquery as cq
        document = ik.read_file(CORPUS / 'INV_nist_ftc_06_asme1_2021.ipt')
        expected = document.convert_bodies(backend='cadquery')
        result = document.convert_bodies(backend='occt')
        self.assertEqual((expected.backend, result.backend), ('cadquery', 'occt'))
        self.assertEqual([b.id for b in result.bodies], [b.id for b in expected.bodies])
        self.assertEqual([b.status for b in result.bodies], [b.status for b in expected.bodies])
        self.assertEqual([b.diagnostics for b in result.bodies], [b.diagnostics for b in expected.bodies])
        self.assertEqual(result.bodies[0].metrics, expected.bodies[0].metrics)
        self.assertIsInstance(expected.bodies[0].shape, cq.Shape)
        self.assertIsInstance(result.bodies[0].shape, TopoDS_Shape)
        self.assertNotIsInstance(result.bodies[0].shape, cq.Shape)
        self.assertEqual({k: v for k, v in result.report().items() if k != 'dependencies'},
                         {k: v for k, v in expected.report().items() if k != 'dependencies'})
        shapes = result.to_occt(body_ids=[result.bodies[0].id], allow_partial=True)
        self.assertEqual(len(shapes), 1)
        self.assertTrue(shapes[0].IsSame(result.bodies[0].shape))
        # The CadQuery result exposes the same OCCT shapes.
        self.assertIsInstance(expected.to_occt(body_ids=[expected.bodies[0].id], allow_partial=True)[0], TopoDS_Shape)
        with self.assertRaises(ik.BodyConversionError):
            result.to_occt()
        for call in (result.to_cadquery, lambda **k: result.export_step('unused.step', **k)):
            with self.assertRaisesRegex(ValueError, 'CadQuery shapes'):
                call(body_ids=[result.bodies[0].id])
        with self.assertRaisesRegex(ValueError, 'unknown backend'):
            document.convert_bodies(backend='mesh')

    def test_document_to_occt_converts_every_body_or_raises(self):
        from OCP.BRepGProp import BRepGProp
        from OCP.GProp import GProp_GProps
        shapes = ik.read_file(CORPUS / 'SamplePart.ipt').to_occt()
        self.assertEqual(len(shapes), 1)
        properties = GProp_GProps()
        BRepGProp.VolumeProperties_s(shapes[0], properties)
        self.assertAlmostEqual(properties.Mass(), 3750, places=6)
        with self.assertRaises(CadQueryConversionError):
            ik.read_file(CORPUS / 'INV_nist_ftc_06_asme1_2021.ipt').to_occt()
        with self.assertRaisesRegex(ValueError, 'not requested'):
            ik.inspect_file(CORPUS / 'SamplePart.ipt').to_occt()

    def test_stored_fits_convert_with_a_warning_on_both_backends(self):
        document = ik.read_file(CORPUS / 'INV_nist_ftc_10_asme1_2024.ipt')
        results = [document.convert_bodies(backend=backend) for backend in ('cadquery', 'occt')]
        for result in results:
            with self.subTest(backend=result.backend):
                body, = result.bodies
                self.assertEqual(body.status, 'converted_solid')
                self.assertEqual(body.diagnostics, ())
                warning, = body.warnings
                self.assertEqual((warning.code, warning.severity),
                                 ('geometry.approximated_from_stored_spline', 'warning'))
                self.assertEqual(len(body.approximations), 22)
                self.assertIn('22 stored B-spline fit(s)', warning.message)
                self.assertTrue(all(a['fit_tolerance_mm'] > 0 for a in body.approximations))
                # A warned body is usable; the report carries the warning.
                self.assertTrue(result.geometry_complete)
                report = result.report()
                self.assertEqual(report['status'], 'success')
                self.assertEqual(report['bodies'][0]['warnings'][0]['code'], warning.code)
                jsonschema.validate(report, json.loads((ROOT/'schemas/conversion-report-v1.schema.json').read_text()))
        self.assertEqual(results[0].bodies[0].metrics, results[1].bodies[0].metrics)
        exact = ik.read_file(CORPUS / 'SamplePart.ipt').convert_bodies()
        self.assertEqual(exact.bodies[0].warnings, ())
        self.assertEqual(exact.report()['bodies'][0]['warnings'], [])
