from dataclasses import FrozenInstanceError
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import Mock

import inventor_kit

ROOT=Path(__file__).resolve().parents[1]
FIXTURES=ROOT/'fixtures/public'
sys.path.insert(0,str(ROOT/'scripts'))
from capture_model_states import select_and_save
from compare_state_candidates import compare_file


class CandidateAPI(unittest.TestCase):
    def test_inventory_explicit_selection_and_original_api(self):
        path=FIXTURES/'Cylinder.ipt'
        shallow=inventor_kit.inspect_file(path)
        self.assertEqual(shallow.geometry.status,'not_scanned')
        self.assertFalse(shallow.geometry.candidates)
        inventory=inventor_kit.inspect_file(path,include_candidates=True)
        self.assertIsNone(inventory.model)
        self.assertIsNone(inventory.kernel_bytes)
        self.assertEqual(inventory.geometry.status,'complete')
        c,=inventory.geometry.candidates
        self.assertEqual(c.table_status,'solved_table')
        self.assertEqual(c.profile_status,'supported_stored_table')
        self.assertEqual(c.header.interpretation,'unresolved')
        self.assertEqual(c.state_binding,'unresolved')
        self.assertEqual(c.kernel_relative_source.byte_domain,'kernel')
        self.assertEqual(c.compressed_source.byte_domain,'cfb_stream')
        self.assertEqual(c.kernel_source.byte_domain,'inflated_stream')
        with self.assertRaises(FrozenInstanceError):c.id='changed'
        doc=inventor_kit.read_file(path,candidate_id=c.id)
        default=inventor_kit.read_file(path)
        self.assertEqual(doc.model,default.model)
        self.assertEqual(doc.kernel_bytes,default.kernel_bytes)
        self.assertEqual(hashlib.sha256(doc.kernel_bytes).hexdigest(),c.kernel_sha256)
        self.assertEqual(doc.geometry.selection.basis,'user_requested_stored_candidate')
        self.assertEqual(default.geometry.selection.basis,'sole_structurally_located_stored_candidate')
        self.assertEqual(doc.geometry.selection.state_verification,'unverified')
        self.assertEqual(len(inventor_kit._inventor.read(path.read_bytes(),'legacy')),3)
        self.assertIsNone(inventor_kit.InventorDocument({},None,None).geometry)

    def test_stale_candidate_and_current_state_never_fall_back(self):
        one=inventor_kit.inspect_file(FIXTURES/'Cylinder.ipt',include_candidates=True).geometry.candidates[0]
        wrong=inventor_kit.read_file(FIXTURES/'SamplePart.ipt',candidate_id=one.id)
        self.assertIsNone(wrong.model)
        self.assertEqual(wrong.geometry.selection.status,'not_found')
        self.assertTrue(wrong.geometry.candidates)
        for candidate_id in [None,one.id]:
            doc=inventor_kit.read_file(FIXTURES/'Cylinder.ipt',candidate_id=candidate_id,require_current_state=True)
            self.assertIsNone(doc.model)
            self.assertEqual(doc.geometry.selection.status,'state_unverified')
            self.assertTrue(doc.metadata.property_sets)
            with self.assertRaisesRegex(ValueError,'Model State'):doc.to_cadquery()

    def test_member_properties_do_not_name_the_geometry_state(self):
        doc=inventor_kit.read_file(FIXTURES/'SamplePart.ipt')
        self.assertEqual(len(doc.geometry.candidates),1)
        self.assertEqual(len(doc.metadata.find_properties(semantic_name='part_number')),3)
        self.assertEqual(doc.geometry.candidates[0].state_binding,'unresolved')
        self.assertEqual(doc.geometry.candidates[0].suppression,'unresolved')
        self.assertEqual(doc.geometry.candidates[0].history_replay,'not_attempted')
        self.assertEqual(doc.metadata.stages.state,'unresolved')

    def test_inventory_does_not_import_python_geometry(self):
        code='''import sys
class DenyGeometry:
    def find_spec(self,fullname,path=None,target=None):
        if fullname.split('.')[0] in ('cq_acis','cadquery','OCP'):
            raise AssertionError(fullname)
sys.meta_path.insert(0,DenyGeometry())
from inventor_kit import inspect_file
d=inspect_file(sys.argv[1],include_candidates=True)
assert d.geometry.candidates and d.model is None
'''
        subprocess.run([sys.executable,'-c',code,str(FIXTURES/'Cylinder.ipt')],check=True)

    def test_cli_inventory_and_strict_state(self):
        for option in ['--list-candidates','--require-current-state']:
            result=subprocess.run([sys.executable,'-m','inventor_kit',str(FIXTURES/'Cylinder.ipt'),option],check=True,capture_output=True,text=True)
            summary=json.loads(result.stdout)
            self.assertEqual(len(summary['geometry']['candidates']),1)
            self.assertEqual(summary['geometry']['selection']['status'],'not_requested' if option=='--list-candidates' else 'state_unverified')

    def test_preparation_requires_successful_activation_and_update_before_save(self):
        # COM replay only; this does not qualify Autodesk or state semantics.
        for failure in ['none','wrong_state','update_failed','requires_update','references']:
            state=SimpleNamespace(Name='wanted' if failure!='wrong_state' else 'different')
            activation=Mock()
            states=SimpleNamespace(ActiveModelState=state,Item=Mock(return_value=SimpleNamespace(Activate=activation)))
            doc=SimpleNamespace(DocumentType=12290,File=SimpleNamespace(ReferencedFileDescriptors=SimpleNamespace(Count=int(failure=='references'))),
                                ComponentDefinition=SimpleNamespace(ModelStates=states),Update2=Mock(return_value=failure!='update_failed'),
                                RequiresUpdate=failure=='requires_update',Save2=Mock())
            if failure=='none':
                select_and_save(doc,'wanted')
                activation.assert_called_once_with();doc.Update2.assert_called_once_with(False);doc.Save2.assert_called_once_with(False)
            else:
                with self.assertRaises(ValueError):select_and_save(doc,'wanted')
                doc.Save2.assert_not_called()

    def test_saved_comparison_without_oracle_does_not_certify_state(self):
        result=compare_file(FIXTURES/'SamplePart.ipt')
        self.assertEqual(result['state_verification'],'unverified')
        self.assertIsNone(result['captured_model_state'])
        self.assertFalse(result['matching_metric_candidates'])
        self.assertIsNone(result['candidates'][0]['comparison'])

    def test_vendor_comparison_rejects_another_source_hash(self):
        with self.assertRaisesRegex(ValueError,'different source SHA-256'):
            compare_file(FIXTURES/'Cylinder.ipt',ROOT/'tests/data/oracle_synthetic.json')

    def test_matching_metrics_identify_candidates_without_certifying_a_state(self):
        source=FIXTURES/'SamplePart.ipt'
        capture=json.loads((ROOT/'tests/data/oracle_synthetic.json').read_text())
        capture['source']=dict(sha256=hashlib.sha256(source.read_bytes()).hexdigest(),bytes=source.stat().st_size,file_name=source.name)
        # Analytic 15 x 10 x 25 box metrics; synthetic provider, not Autodesk.
        capture['geometry'].update(volume_mm3=3750,area_mm2=1550,bbox_kind='unknown')
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'synthetic.json'
            path.write_text(json.dumps(capture))
            result=compare_file(source,path)
            self.assertEqual(len(result['matching_metric_candidates']),1)
            self.assertEqual(result['state_verification'],'unverified')
            self.assertEqual(result['candidates'][0]['comparison']['state_verification'],'unverified')
            capture['geometry']['volume_mm3']=7500
            path.write_text(json.dumps(capture))
            result=compare_file(source,path)
            self.assertFalse(result['matching_metric_candidates'])
            self.assertEqual(result['candidates'][0]['comparison']['status'],'mismatch')


if __name__=='__main__':unittest.main()
