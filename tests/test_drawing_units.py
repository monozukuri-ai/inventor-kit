"""Save-boundary safety tests; synthetic state is not Inventor evidence."""
import copy
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from validate_drawing_unit_controls import saved_boundary, local_file
from drawing_oracle_contract import identity


class UnitEvidence(unittest.TestCase):
    def test_only_explicit_unchanged_saved_state_is_admitted(self):
        state = dict(dirty=False, requires_update=False, defer_updates=False, needs_migrating=False,
                     sheet_status=[0], revision='synthetic-revision', file_save_counter=1)
        row = dict(capture_scope='freshly_saved_document', reopened_during_capture=False,
                   save_requested_during_capture=False, update_requested_during_capture=False,
                   before=copy.deepcopy(state), after=copy.deepcopy(state), sheet={'width_cm': 29.7},
                   sheet_after={'width_cm': 29.7}, source={'sha256': 'synthetic'}, source_after={'sha256': 'synthetic'})
        saved_boundary(row)
        for label in ('before', 'after'):
            for key in ('dirty', 'requires_update', 'defer_updates', 'needs_migrating'):
                for bad in (True, None, 0, 'False'):
                    changed=copy.deepcopy(row); changed[label][key]=bad
                    with self.subTest(label=label,key=key,bad=bad), self.assertRaises(ValueError):
                        saved_boundary(changed)
        changes = [('capture_scope','opened_document'), ('reopened_during_capture',True),
                   ('save_requested_during_capture',True), ('update_requested_during_capture',True),
                   ('sheet_after',{'width_cm':30}), ('source_after',{'sha256':'changed'})]
        for key, value in changes:
            changed=copy.deepcopy(row); changed[key]=value
            with self.subTest(key=key), self.assertRaises(ValueError): saved_boundary(changed)
        changed=copy.deepcopy(row); changed['after']['revision']='another revision'
        with self.assertRaises(ValueError): saved_boundary(changed)
        for value in ([False], [], [1], [0, 0]):
            changed=copy.deepcopy(row); changed['before']['sheet_status']=value
            with self.assertRaises(ValueError): saved_boundary(changed)

    def test_hash_suffix_traversal_and_symlink_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp); path=root/'test.idw'; path.write_bytes(b'synthetic')
            record=identity(path)
            self.assertEqual(local_file(root,record,'.idw'),path)
            with self.assertRaises(ValueError): local_file(root,record,'.pdf')
            for bad in ('../test.idw', '/test.idw', 'folder\\test.idw'):
                with self.assertRaises(ValueError): local_file(root,record|{'file_name':bad},'.idw')
            path.write_bytes(b'changed')
            with self.assertRaises(ValueError): local_file(root,record,'.idw')


class HoldoutEvidence(unittest.TestCase):
    def test_acquisition_verifies_originals_and_never_decodes_reserved_inputs(self):
        import hashlib
        import json
        from unittest.mock import patch
        import validate_drawing_holdouts as holdouts
        with tempfile.TemporaryDirectory() as tmp:
            base=Path(tmp); repo=base/'repo'; output=base/'capture'
            (repo/'fixtures').mkdir(parents=True); (repo/'scripts').mkdir()
            script=repo/'scripts/capture_drawing_holdouts.ps1'; script.write_bytes(b'synthetic collector')
            assets=[]; rows=[]
            for family in sorted(holdouts.FAMILIES):
                original=output/family/'original'; working=output/family/'major31'
                original.mkdir(parents=True); working.mkdir()
                source=original/'original.idw'; source.write_bytes(b'synthetic original')
                saved=working/'major31.idw'; saved.write_bytes(b'synthetic saved snapshot')
                pdf=working/'major31.pdf'; pdf.write_bytes(b'synthetic PDF identity only')
                url='https://example.invalid/'+family
                assets.append(dict(file=family+'/original.idw',family_id=family,url=url,**{k:v for k,v in identity(source).items() if k!='file_name'}))
                state=dict(dirty=False, requires_update=False, defer_updates=False, needs_migrating=False,
                           sheet_status=[0], revision='synthetic-revision', file_save_counter=1)
                sheets=[dict(views=[dict(up_to_date=dict(status='captured',value=True),curve_count=dict(status='captured',value=1))])]
                rows.append(dict(family_id=family,split='holdout',status='captured',source=identity(saved),source_after=identity(saved),pdf=identity(pdf),
                    inputs=[dict(upstream_path=family+'/original.idw',url=url,identity=identity(source))],
                    capture_scope='freshly_saved_document',reopened_during_capture=False,save_requested_during_capture=False,
                    update_requested_during_capture=False,before=state,after=copy.deepcopy(state),sheets=sheets,sheets_after=copy.deepcopy(sheets),references=[]))
            (repo/'fixtures/drawing-manifest.json').write_text(json.dumps(dict(assets=assets)))
            report=dict(format='inventor-kit-migrated-holdouts-v1',qualified_oracle=False,rows=rows,
                        script=dict(sha256=hashlib.sha256(script.read_bytes()).hexdigest()))
            path=output/'holdouts.native.json'; path.write_text(json.dumps(report))
            with patch.object(holdouts,'ROOT',repo), patch('inventor_kit.read_drawing_file',side_effect=AssertionError('holdouts must stay undecoded')):
                result=holdouts.validate(output)
                self.assertEqual(result['status'],'acquisition_checks_passed')
                self.assertEqual(result['independent_qualified_holdout_families'],[])
                self.assertFalse(result['qualified_oracle'])
                for change in ('hash','getter','stale','provenance','missing_reference'):
                    changed=copy.deepcopy(report); row=changed['rows'][0]
                    if change=='hash': row['source']['sha256']='0'*64; row['source_after']=copy.deepcopy(row['source'])
                    if change=='getter': row['sheets'][0]['views'][0]['curve_count']=dict(status='failed',reason='synthetic failure'); row['sheets_after']=copy.deepcopy(row['sheets'])
                    if change=='stale': row['sheets'][0]['views'][0]['up_to_date']['value']=False; row['sheets_after']=copy.deepcopy(row['sheets'])
                    if change=='provenance': row['inputs'][0]['url']='https://example.invalid/wrong'
                    if change=='missing_reference': row['references']=[dict(missing=True)]
                    path.write_text(json.dumps(changed))
                    with self.subTest(change=change),self.assertRaises(ValueError): holdouts.validate(output)
