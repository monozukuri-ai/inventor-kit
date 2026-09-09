from __future__ import annotations

from dataclasses import FrozenInstanceError, replace
from decimal import Decimal, localcontext
import io
import json
from pathlib import Path
import subprocess
import struct
import sys
import unittest

import inventor_kit
import olefile
from inventor_kit.document import _value

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT/'fixtures/public'


def mutate_stream(data, path, mutate):
    buffer = io.BytesIO(data)
    with olefile.OleFileIO(buffer, write_mode=True) as ole:
        value = bytearray(ole.openstream(path).read())
        mutate(value)
        ole.write_stream(path, bytes(value))
        # olefile 0.46 closes caller-owned buffers when leaving the context.
        return buffer.getvalue()


class DocumentAPI(unittest.TestCase):
    def test_inspection_all_document_kinds_and_raw_provenance(self):
        for name,kind in [('Cylinder.ipt','part'),('BoltedConnection.iam','assembly'),('SampleBg.idw','drawing'),('_Fishing Reel Assembly.ipn','presentation')]:
            with self.subTest(name=name):
                data=(FIXTURES/name).read_bytes()
                doc=inventor_kit.inspect(data,source_id='renamed.bin')
                self.assertEqual(doc.metadata.identification.kind,kind)
                self.assertEqual(doc.metadata.stages.geometry,'not_attempted')
                self.assertIsNone(doc.model)
                self.assertIsNone(doc.kernel_bytes)
                props=doc.metadata.find_properties(semantic_name='part_number')
                self.assertEqual(len(props),1)
                self.assertEqual(props[0].type_code,31)
                self.assertEqual(props[0].state_binding,'unresolved')
                self.assertEqual(doc.metadata.stages.state,'unresolved')
                with self.assertRaises(FrozenInstanceError):props[0].value='changed'
                with olefile.OleFileIO(io.BytesIO(data)) as ole:
                    for s in doc.metadata.property_sets:
                        raw=ole.openstream(s.stream.lstrip('/')).read()
                        for prop in s.properties:
                            self.assertEqual(prop.raw_data,raw[prop.source.start_offset:prop.source.end_offset])
                    thumb=doc.metadata.thumbnails[0]
                    raw=ole.openstream(thumb.source.stream.lstrip('/')).read()
                    self.assertEqual(thumb.data,raw[thumb.source.start_offset:thumb.source.end_offset])
                    self.assertEqual(thumb.mime_type,'image/png')
                with self.assertRaisesRegex(ValueError,'Geometry was not requested'):doc.to_cadquery()

    def test_no_optional_geometry_import_in_fresh_process(self):
        source = '''import sys
class DenyGeometry:
    def find_spec(self, fullname, path=None, target=None):
        if fullname.split('.')[0] in ('cq_acis','cadquery','OCP'):
            raise AssertionError('geometry import: ' + fullname)
sys.meta_path.insert(0,DenyGeometry())
import inventor_kit
x=inventor_kit.inspect_file(sys.argv[1])
assert x.metadata.property_sets and x.model is None
assert 'cq_acis' not in sys.modules
'''
        subprocess.run([sys.executable,'-c',source,str(FIXTURES/'Cylinder.ipt')],check=True)

    def test_metadata_cli_uses_independent_inspection(self):
        result=subprocess.run([sys.executable,'-m','inventor_kit',str(FIXTURES/'SampleBg.idw'),'--metadata-only'],check=True,capture_output=True,text=True)
        summary=json.loads(result.stdout)
        self.assertEqual(summary['document']['stages']['geometry'],'not_attempted')
        self.assertEqual(summary['document']['identification']['kind'],'drawing')
        self.assertTrue(summary['document']['property_sets'])

    def test_unknown_database_schema_keeps_properties(self):
        data=(FIXTURES/'Cylinder.ipt').read_bytes()
        with olefile.OleFileIO(io.BytesIO(data)) as ole:
            path=next(p for p in ole.listdir() if p[-1]=='RSeDb')
        bad=mutate_stream(data,path,lambda b:b.__setitem__(slice(16,20),struct.pack('<I',999)))
        for doc in [inventor_kit.inspect(bad),inventor_kit.read(bad)]:
            self.assertEqual(doc.metadata.identification.kind,'part')
            self.assertEqual(doc.metadata.find_properties(semantic_name='part_number')[0].value,'Cylinder')
            self.assertEqual(doc.metadata.databases[0].schema,999)
            self.assertIsNone(doc.model)
            self.assertTrue(any(d.code=='rse.database_unavailable' and d.source for d in doc.metadata.diagnostics))

    def test_bad_property_does_not_break_other_properties_or_geometry(self):
        data=(FIXTURES/'Cylinder.ipt').read_bytes()
        original=inventor_kit.inspect(data)
        prop=original.metadata.find_properties(semantic_name='part_number')[0]
        bad=mutate_stream(data,prop.source.stream.lstrip('/'),lambda b:b.__setitem__(slice(prop.source.start_offset+4,prop.source.start_offset+8),struct.pack('<I',0xffff_ffff)))
        doc=inventor_kit.read(bad)
        bad_prop=doc.metadata.find_properties(semantic_name='part_number')[0]
        self.assertEqual(bad_prop.status,'malformed')
        self.assertIsNone(bad_prop.value)
        self.assertIsNotNone(doc.model)
        self.assertEqual(doc.metadata.stages.properties,'partial')
        self.assertTrue(any(d.code=='property.value_invalid' and d.pid==5 and d.fmtid==prop.fmtid for d in doc.metadata.diagnostics))

    def test_thumbnail_crc_failure_is_not_geometry_failure(self):
        data=(FIXTURES/'Cylinder.ipt').read_bytes()
        thumb=inventor_kit.inspect(data).metadata.thumbnails[0]
        def corrupt(b):b[thumb.source.start_offset+29]^=1
        bad=mutate_stream(data,thumb.source.stream.lstrip('/'),corrupt)
        doc=inventor_kit.read(bad)
        self.assertIsNotNone(doc.model)
        self.assertFalse(doc.metadata.thumbnails)
        self.assertTrue(any(d.code=='thumbnail.unsupported_or_invalid' for d in doc.metadata.diagnostics))

    def test_ambiguous_property_owners_are_returned_without_selection(self):
        info=inventor_kit.inspect_file(FIXTURES/'Cylinder.ipt').metadata
        tracking=next(s for s in info.property_sets if s.fmtid=='32853f0f-3444-11d1-9e93-0060b03c1ca6')
        duplicate=replace(info,property_sets=info.property_sets+(tracking,))
        matches=duplicate.find_properties(fmtid='{32853F0F-3444-11D1-9E93-0060B03C1CA6}',pid=5)
        self.assertEqual(len(matches),2)
        self.assertTrue(all(p.state_binding=='unresolved' for p in matches))

    def test_member_metadata_is_not_collapsed_into_effective_state(self):
        info=inventor_kit.inspect_file(FIXTURES/'SamplePart.ipt').metadata
        numbers=info.find_properties(semantic_name='part_number')
        self.assertEqual(len(numbers),3)
        self.assertEqual(len({p.source.stream for p in numbers}),3)
        self.assertEqual({p.storage_scope for p in numbers},{'document_storage','nested_storage'})
        projects=info.find_properties(fmtid='32853f0f-3444-11d1-9e93-0060b03c1ca6',pid=7)
        arrays=[p for p in projects if p.value_kind=='sequence']
        self.assertEqual(len(arrays),2)
        self.assertEqual({p.value.values for p in arrays},{('Proj2','Proj1'),('Proj3','Proj1')})
        self.assertTrue(all(p.state_binding=='unresolved' for p in projects))
        self.assertEqual(len(info.thumbnails),3)

    def test_typed_time_decimal_and_binary_do_not_lose_precision(self):
        time=_value({'kind':'filetime','ticks_100ns':123456789})
        self.assertEqual(time.ticks_100ns,123456789)
        self.assertEqual(time.datetime_utc.microsecond,345678)
        self.assertIsNone(_value({'kind':'filetime','ticks_100ns':2**64-1}).datetime_utc)
        with localcontext() as context:
            context.prec=3
            self.assertEqual(_value({'kind':'currency','scaled_value':9223372036854775807}),Decimal('922337203685477.5807'))
            self.assertEqual(_value({'kind':'decimal','high':0,'low':123456789,'scale':4,'negative':True}),Decimal('-12345.6789'))
        self.assertEqual(_value({'kind':'blob','data':'0080ff'}),b'\x00\x80\xff')


if __name__=='__main__':unittest.main()
