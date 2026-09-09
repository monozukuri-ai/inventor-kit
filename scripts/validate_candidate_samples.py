"""Cross-check candidate framing with Python/olefile; never certify Model State ownership."""
from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import io
import json
from pathlib import Path
import struct
import uuid
import zlib

import inventor_kit
import olefile
import zstandard
from corpus_manifest import load_manifest

ROOT=Path(__file__).resolve().parents[1]
KERNEL='f645595c-11d5-1333-1000-60a6bba647b5'


class Wire:
    def __init__(self,data):self.data,self.pos=data,0
    def take(self,n):
        value=self.data[self.pos:self.pos+n]
        if n<0 or len(value)!=n:raise ValueError('truncated independent wire read')
        self.pos+=n
        return value
    def unpack(self,fmt):return struct.unpack(fmt,self.take(struct.calcsize(fmt)))
    def u32(self):return self.unpack('<I')[0]
    def text(self,wide=False):
        n=self.u32()
        return self.take(n*(2 if wide else 1)).decode('utf-16le' if wide else 'utf-8')


def inflate(data):
    if data.startswith(b'\x28\xb5\x2f\xfd'):
        return zstandard.ZstdDecompressor().decompress(data,max_output_size=64*1024*1024), 'zstd'
    d=zlib.decompressobj()
    out=d.decompress(data,64*1024*1024+1)
    assert d.eof and not d.unused_data and not d.unconsumed_tail and len(out)<=64*1024*1024
    return out,'zlib'


def metadata(data):
    r=Wire(data)
    assert r.text()=='RSe Meta Stream Version 8' and r.unpack('<H')==(8,)
    r.take(16);name=r.text(True);identity=str(uuid.UUID(bytes_le=r.take(16)))
    states=r.unpack('<III');r.text();r.text();r.take(1)
    offset=r.pos;expanded,codec=inflate(data[offset:])
    r=Wire(expanded);r.take(14)
    blocks=types=None
    for section,size in enumerate([4,10,28,28]):
        n=r.u32();payload=r.take(n*size)
        assert r.u32()==4+n*size
        if section==0:blocks=struct.unpack('<'+'I'*n,payload)
        if section==3:types=[str(uuid.UUID(bytes_le=payload[i:i+16])) for i in range(0,len(payload),28)]
    return dict(name=name,identity=identity,states=states,offset=offset,codec=codec,expanded_bytes=len(expanded),blocks=blocks,types=types)


def frames(data,meta,major):
    r=Wire(data);records=[]
    for ordinal,block in enumerate(meta['blocks']):
        if not block & 0x80000000:continue
        tag=r.u32();kind=meta['types'][tag&255];n=block&0x7fffffff
        start=r.pos;payload=r.take(n);end=r.pos
        assert r.u32() in (0,n)
        if major>18 and r.take(1)!=b'\0':
            count=r.u32()
            if not count & 0x80000000:
                for _ in range(count):
                    r.text();t=r.u32()
                    if t==14:r.take(2);r.take(r.u32())
                    else:r.take({1:3,3:4,7:4,8:6,10:6,11:10}[t])
                assert r.unpack('<HH')==(6,0x3000)
                refs=r.u32()
                if refs:
                    r.take(8)
                    for _ in range(refs):r.text();r.take(4)
        records.append(dict(ordinal=ordinal,kind=kind,start=start,end=end,payload=payload,tag=tag))
    assert r.u32()==0xffffffff
    return records


def probe(value,records):
    # A research hypothesis only. A numeric hit is not an authoritative pointer.
    ordinal=(value&0x7fffffff)-1 if value!=0xffffffff and value&0x80000000 else None
    hits=[r for r in records if r['ordinal']==ordinal]
    return dict(raw=value,hypothesis='high_bit_tagged_one_based_ordinal',hypothetical_ordinal=ordinal,
                target_type=hits[0]['kind'] if len(hits)==1 else None,semantics='unverified')


def compare(item,fixtures):
    data=(fixtures/item['file']).read_bytes();sha=hashlib.sha256(data).hexdigest()
    assert len(data)==item['bytes'] and sha==item['sha256']
    doc=inventor_kit.inspect(data,source_id=item['file'],include_candidates=True)
    assert doc.geometry.source_sha256==sha and doc.model is None and doc.kernel_bytes is None
    assert doc.geometry.status=='complete'
    candidate_report=[];expected=set()
    with olefile.OleFileIO(io.BytesIO(data)) as ole:
        paths={'/'+'/'.join(p) for p in ole.listdir()}
        expected_pairs={p[len('/RSeStorage/')+1:] for p in paths if p.startswith(('/RSeStorage/M','/RSeStorage/B')) and p.count('/')==2}
        assert expected_pairs=={s.token for s in doc.geometry.segments}
        for segment in doc.geometry.segments:
            if not segment.meta_source:continue
            raw=ole.openstream(segment.meta_source.stream.lstrip('/')).read()
            head=Wire(raw);assert head.text()=='RSe Meta Stream Version 8' and head.unpack('<H')==(8,)
            head.take(16);name=head.text(True);identity=str(uuid.UUID(bytes_le=head.take(16)))
            assert (name,identity)==(segment.name,segment.segment_id)
            assert segment.registry_indices==tuple(i for i,r in enumerate(doc.metadata.segments) if r.id==identity)
            if segment.status!='framed':continue
            meta=metadata(raw)
            assert (meta['states'],meta['offset'],meta['codec'],meta['expanded_bytes'])==(segment.meta_state_words,segment.meta_compressed_source.start_offset,segment.meta_codec,segment.meta_expanded_bytes)
            reg=doc.metadata.segments[segment.registry_indices[0]]
            compressed=ole.openstream(segment.bulk_source.stream.lstrip('/')).read()
            bulk,codec=inflate(compressed[18:]);records=frames(bulk,meta,reg.major)
            assert len(records)==segment.record_count
            for record in records:
                if record['kind']!=KERNEL:continue
                expected.add((segment.token,record['ordinal']))
                matches=[c for c in doc.geometry.candidates if c.segment_index==doc.geometry.segments.index(segment) and c.record_ordinal==record['ordinal']]
                assert len(matches)==1
                c=matches[0];carrier=c.carrier;footer=17 if reg.major<23 else 18
                start,end=record['start']+14,record['end']-footer
                assert (c.record_source.start_offset,c.record_source.end_offset)==(record['start'],record['end'])
                assert (c.kernel_source.start_offset,c.kernel_source.end_offset)==(start,end)
                assert (c.compressed_source.start_offset,c.compressed_source.end_offset)==(18,len(compressed))
                kernel=bulk[start:end];assert hashlib.sha256(kernel).hexdigest()==c.kernel_sha256
                assert (c.kernel_relative_source.start_offset,c.kernel_relative_source.end_offset)==(0,len(kernel))
                head=struct.unpack('<IHII',bulk[record['start']:start]);assert head==(c.header.state,c.header.kind,c.header.value,c.header.schema)
                f=Wire(bulk[end:record['end']]);key=f.u32();enabled=f.take(1)[0];delta=f.unpack('<i')[0]
                if reg.major>=23:assert f.take(1)==b'\0'
                history=f.u32();assert f.u32()==0xffffffff
                assert (key,enabled,delta,history)==(carrier.selected_key,carrier.enabled,carrier.delta_state,carrier.history_reference)
                assert (carrier.codec,carrier.expanded_bytes)==(codec,len(bulk))
                assert c.table_status=='solved_table' and c.profile_status=='supported_stored_table'
                assert c.state_binding==c.database_binding==c.suppression=='unresolved'
                assert c.history_replay=='not_attempted'
                candidate_report.append(dict(id=c.id,kernel_sha256=c.kernel_sha256,record_ordinal=c.record_ordinal,
                    profile=dict(rse_db=31,meta=8,segment=reg.major,carrier_schema=c.header.schema,sab=carrier.save_version),
                    header=asdict(c.header),meta_state_words=segment.meta_state_words,
                    footer=dict(selected_key=key,enabled=bool(enabled),delta_state=delta,history_reference=history),
                    selected_key_probe=probe(key,records),history_reference_probe=probe(history,records),
                    sources={k:asdict(getattr(c,k)) if getattr(c,k) else None for k in ['compressed_source','record_source','kernel_source','kernel_relative_source','solved_source','history_source']},
                    solved_records=c.solved_records,state_binding='unresolved'))
        assert expected=={(doc.geometry.segments[c.segment_index].token,c.record_ordinal) for c in doc.geometry.candidates}
    strict=inventor_kit.read(data,source_id=item['file'],require_current_state=True)
    assert strict.model is None and strict.geometry.selection.state_verification=='unverified'
    return dict(file=item['file'],sha256=sha,split=item['split'],kind=doc.metadata.identification.kind,
                databases=len(doc.metadata.databases),segment_pairs=len(doc.geometry.segments),candidates=candidate_report,
                strict_state_status=strict.geometry.selection.status,state_verification='unverified')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--split',choices=['all','regression','holdout'],default='all')
    parser.add_argument('--fixtures',type=Path,default=ROOT/'fixtures/public')
    parser.add_argument('--output',type=Path,default=ROOT/'internal/reports/latest/candidate_validation.json')
    args=parser.parse_args();results=[]
    for item in load_manifest(split=args.split):
        result=compare(item,args.fixtures);results.append(result)
        print(item['file'],result['segment_pairs'],'pairs,',len(result['candidates']),'typed candidates',flush=True)
    report=dict(scope='Independent CFB/decompression and Python wire cross-check; no vendor state oracle',
                providers=dict(olefile=olefile.__version__,zstandard=zstandard.__version__),
                by_split={split:dict(files=sum(r['split']==split for r in results),candidates=sum(len(r['candidates']) for r in results if r['split']==split)) for split in ['regression','holdout']},
                profiles=dict(Counter('/'.join(str(v) for v in c['profile'].values()) for r in results for c in r['candidates'])),
                limitations=['Multiple/suppressed states are not vendor-qualified. Controlled mutations are not real Model State saves.',
                             'Reference probes are hypotheses only. Numeric hits do not establish pointer semantics or current-state ownership.',
                             'Saved-table decoding does not replay SAB history or prove current feature evaluation.'],results=results)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2,allow_nan=False)+'\n')


if __name__=='__main__':main()
