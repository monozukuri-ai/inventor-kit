"""Independent wire-range checks for the structural report, using synthetic bytes."""
import copy
from contextlib import contextmanager
import hashlib
import io
import json
from pathlib import Path
import sys
import struct
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'scripts'))
from drawing_corpus import drawing_rows
from validate_drawing_inventory import verify_sources, verify_field


def span(stream,a,b,domain='inflated_stream'):
    return dict(source_id='synthetic',stream=stream,start_offset=a,end_offset=b,byte_domain=domain)


class DrawingInventory(unittest.TestCase):
    def test_baseline_is_regression_only_and_never_claims_semantic_truth(self):
        baseline=json.loads((ROOT/'tests/data/drawing-inventory-baseline.json').read_text())
        rows={row['sha256']:row for _,row in drawing_rows()}
        self.assertFalse(baseline['holdouts_used'])
        self.assertEqual(baseline['scope'],'structural_regression_not_independent_drawing_oracle')
        self.assertEqual(len(baseline['results']),sum(r['split']=='regression' for r in rows.values()))
        for item in baseline['results']:
            row=rows[item['sha256']]
            self.assertEqual(row['split'],'regression')
            self.assertEqual(Path(row['file']).name,item['file'])
        totals = {major: sum(s['framed_records'] for r in baseline['results']
                            for s in r['segments'] if s['major'] == major)
                  for major in (23, 24, 26, 28, 29, 31)}
        self.assertEqual(totals, {23: 112352, 24: 6901, 26: 14147, 28: 15708, 29: 3404, 31: 5733})
        self.assertEqual(sum(sum(r['types'].values()) for r in baseline['results']),158245)

    def test_typed_fields_match_wire_bytes_without_promoting_semantics(self):
        text = '図面\0𝄞'.encode('utf-16le')
        values = [
            (dict(encoding='utf16le_counted', raw='図面\0𝄞'), struct.pack('<I', len(text)//2)+text),
            (dict(encoding='f32le', raw=[0.1, -2.5]), struct.pack('<2f', 0.1, -2.5)),
            (dict(encoding='f64le', raw=[1.0, -0.0]), struct.pack('<2d', 1.0, -0.0)),
            (dict(encoding='u8', raw=1), b'\x01'),
            (dict(encoding='u16le', raw=9), struct.pack('<H', 9)),
            (dict(encoding='u32le', raw=[0x80000001]), struct.pack('<I', 0x80000001)),
        ]
        matrix = [1.,0.,0.,5.,0.,1.,0.,-7.,0.,0.,1.,0.,0.,0.,0.,1.]
        for prefixed in (False, True):
            values.append((dict(encoding='compact_transform',raw=dict(prefixed=prefixed,
                set=0x8421,zero=0x7b56,values=matrix)),
                (struct.pack('<I',0x203) if prefixed else b'')+struct.pack('<HH2d',0x8421,0x7b56,5.,-7.)))
        for value, data in values:
            verify_field(value, data)
            with self.assertRaises((ValueError, struct.error, UnicodeDecodeError)):
                verify_field(value, data+b'\0')
        with self.assertRaises(ValueError):
            verify_field(dict(encoding='f64le',raw=[float('nan')]),struct.pack('<d',float('nan')))

    def test_payload_type_reference_and_compressed_ranges_bind_to_bytes(self):
        type_bytes=bytes(range(16));guid=str(uuid.UUID(bytes_le=type_bytes))
        meta_bytes=(0x80000003).to_bytes(4,'little')+type_bytes+bytes(range(32))
        bulk_bytes=bytes(4)+b'abc'+(3).to_bytes(4,'little')+b'\0'+b'\xff'*4
        stored={'M':zlib.compress(meta_bytes),'B':zlib.compress(bulk_bytes),'R':b'r'}
        meta=dict(compressed_source=span('M',0,len(stored['M']),'cfb_stream'),expanded_bytes=len(meta_bytes),codec='zlib',
                  reference_tables=[dict(section=7,count=1,bytes=list(range(32)),source=span('M',20,52))],
                  block_words=[0x80000003],block_table_source=span('M',0,4),types=[dict(type_id=guid,source=span('M',4,20))])
        bulk=dict(compressed_source=span('B',0,len(stored['B']),'cfb_stream'),expanded_bytes=len(bulk_bytes),codec='zlib',terminal_source=span('B',12,16))
        record=dict(ordinal=0,type_id=guid,selector_word=0,type_index=0,meta_block_source=span('M',0,4),
                    source=span('B',4,7),selector_source=span('B',0,4),trailer_source=span('B',7,12),references=[])
        segment=dict(status='framed',registry=dict(source=span('R',0,1,'cfb_stream')),meta=meta,bulk=bulk,records=[record],opaque_regions=[])
        doc=dict(source_id='synthetic',drawing_semantics='not_decoded',sheet_count=None,segments=[segment],unclaimed_streams=[])
        @contextmanager
        def cfb(_):
            yield SimpleNamespace(openstream=lambda name:io.BytesIO(stored[name]))
        with tempfile.TemporaryDirectory() as directory,patch('olefile.OleFileIO',cfb):
            source=Path(directory)/'synthetic';source.write_bytes(b'not a native CFB')
            doc['source_sha256']=hashlib.sha256(source.read_bytes()).hexdigest()
            self.assertEqual(verify_sources(doc,source),1)
            changes=[lambda d:d['segments'][0]['meta']['reference_tables'][0].update(count=2),
                     lambda d:d['segments'][0]['meta']['reference_tables'][0]['bytes'].__setitem__(0,99),
                     lambda d:d.update(sheet_count=1),
                     lambda d:d.update(source_sha256='0'*64),
                     lambda d:d['segments'][0]['records'][0].update(type_id='0'*36),
                     lambda d:d['segments'][0]['records'][0]['source'].update(end_offset=8),
                     lambda d:d['segments'][0]['records'][0].update(ordinal=1),
                     lambda d:d['segments'][0]['meta']['compressed_source'].update(start_offset=-1),
                     lambda d:d['segments'][0]['meta']['types'][0]['source'].update(end_offset=999),
                     lambda d:d['segments'][0].update(status='unavailable')]
            for mutate in changes:
                modified=copy.deepcopy(doc);mutate(modified)
                with self.assertRaises(ValueError):
                    verify_sources(modified,source)


if __name__=='__main__':
    unittest.main()
