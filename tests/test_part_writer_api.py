"""Synthetic public writer tests; these do not establish Inventor acceptance."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import inventor_kit as ik
from inventor_kit import _inventor


class WriterContract(unittest.TestCase):
    def test_profile_units_and_names(self):
        for kw in ({'units':'inch'}, {'target_profile':'unknown'}, {'name':''}, {'title':'\0'},
                   {'part_number':'x'*256}, {'name':'\ud800'}):
            with self.subTest(kw=kw), self.assertRaises(ik.PartWriteError):
                ik.new_part(**({'name':'ok'} | kw))

    def test_empty_is_not_a_document(self):
        with tempfile.TemporaryDirectory() as d, self.assertRaises(ik.PartWriteError):
            ik.new_part(name='empty').saveas(Path(d)/'empty.ipt')


class WriterIntegration(unittest.TestCase):
    def setUp(self):
        import cadquery as cq
        self.cq=cq
        self.tmp=tempfile.TemporaryDirectory()
        self.root=Path(self.tmp.name)
        self.shape=cq.Workplane('XY').box(20,30,10,centered=False).val()

    def tearDown(self):
        self.tmp.cleanup()

    def part(self,shape=None):
        return ik.new_part(name='部品',title='検証タイトル',part_number='PN-01日本語').add_body(
            self.shape if shape is None else shape, name='本体',feature_name='基本形状')

    def test_round_trip_geometry_and_metadata(self):
        for i,shape in enumerate((self.shape,self.cq.Workplane('XY').circle(10).extrude(20).val(),
                self.shape.cut(self.cq.Workplane('XY').center(5,5).circle(2).extrude(10).val()).Solids()[0])):
            with self.subTest(i=i):
                report=self.part(shape).saveas(self.root/f'形状{i}.ipt')
                doc=ik.read_file(report.path);result=doc.to_cadquery().val()
                self.assertTrue(result.isValid());self.assertEqual(len(result.Solids()),1)
                self.assertAlmostEqual(result.Volume(),shape.Volume(),places=6)
                self.assertEqual(len(result.Faces()),len(shape.Faces()))
                props={p.semantic_name:p.value for s in doc.metadata.property_sets for p in s.properties if p.semantic_name}
                self.assertEqual(props,{'title':'検証タイトル','part_number':'PN-01日本語'})
                sidecar=json.loads(report.report_path.read_text())
                self.assertEqual(report.sha256,hashlib.sha256(report.path.read_bytes()).hexdigest())
                self.assertEqual(sidecar['native_validation'],'not_run')
                self.assertEqual(report.native_validation,'not_run')
                self.assertEqual(sidecar['profile_qualification'],'native-qualified-bounded-corpus')
                self.assertEqual(sidecar['profile_qualification_scope']['inventor_build'],'311270010')
                self.assertTrue(sidecar['profile_qualification_scope']['development_build_only'])

    def test_rejects_second_body(self):
        with self.assertRaises(ik.PartWriteError): self.part().add_body(self.shape)

    def test_rejects_non_solid_and_unsupported_surfaces(self):
        for shape in (self.cq.Workplane('XY').circle(3).val(),self.cq.Solid.makeSphere(3),self.shape.wrapped):
            with self.subTest(shape=type(shape)), self.assertRaises(ik.PartWriteError):
                self.part(shape)
        self.assertEqual(list(self.root.iterdir()),[])

    def test_fresh_identity_and_input_snapshot(self):
        part=self.part();a=part.saveas(self.root/'a.ipt');b=part.saveas(self.root/'b.ipt')
        da,db=ik.read_file(a.path),ik.read_file(b.path)
        self.assertNotEqual(a.sha256,b.sha256)
        self.assertTrue(set(s.id for s in da.metadata.segments).isdisjoint(s.id for s in db.metadata.segments))

    def test_existing_ipt_report_and_dangling_symlink_are_preserved(self):
        part=self.part()
        for suffix in ('.ipt','.ipt.json'):
            path=self.root/'target.ipt';existing=Path(str(path)+('.json' if suffix.endswith('json') else ''))
            existing.write_bytes(b'keep')
            with self.assertRaises(FileExistsError):part.saveas(path)
            self.assertEqual(existing.read_bytes(),b'keep');existing.unlink()
        path=self.root/'link.ipt';path.symlink_to(self.root/'absent')
        with self.assertRaises(FileExistsError):part.saveas(path)
        self.assertTrue(path.is_symlink())

    def test_sidecar_race_rolls_back_only_own_ipt(self):
        import os
        real=os.link;path=self.root/'race.ipt';sidecar=Path(str(path)+'.json')
        def raced(src,dst):
            if Path(dst)==sidecar:sidecar.write_bytes(b'other writer')
            return real(src,dst)
        with patch('inventor_kit.writer.os.link',side_effect=raced),self.assertRaises(FileExistsError):
            self.part().saveas(path)
        self.assertFalse(path.exists());self.assertEqual(sidecar.read_bytes(),b'other writer')
        self.assertEqual(list(self.root.iterdir()),[sidecar])

    def test_native_boundary_rejects_raw_payload_and_bad_mesh(self):
        part=self.part();body=json.loads(part._body_json)
        spec=dict(profile='inventor-ipt-rse31-metric-analytic-base-feature-v21',solid=body['solid'],
            display=body['display'],metadata={**part._metadata,'body_name':'B','feature_name':'F'})
        spec['sab']='untrusted'
        with self.assertRaises(ValueError):_inventor.encode_analytic_part(json.dumps(spec))
        del spec['sab'];spec['display']['faces'][0]['triangles'][0]=[999999,1,2]
        with self.assertRaises(ValueError):_inventor.encode_analytic_part(json.dumps(spec))


if __name__=='__main__':unittest.main()
