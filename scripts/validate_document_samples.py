"""Compare metadata with independent olefile reads and PNG pixels with Pillow."""
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

import inventor_kit
import olefile
from PIL import Image
from corpus_manifest import load_manifest

ROOT = Path(__file__).resolve().parents[1]


def compare(item, directory):
    path = directory / item['file']
    data = path.read_bytes()
    if len(data) != item['bytes'] or hashlib.sha256(data).hexdigest() != item['sha256']:
        raise ValueError(f'Fixture hash/size mismatch: {path}')
    doc = inventor_kit.inspect(data, source_id=item['file'])
    metadata = doc.metadata
    assert doc.model is None and doc.kernel_bytes is None
    assert metadata.stages.geometry == 'not_attempted'
    assert metadata.stages.state == 'unresolved'
    matched, binary, times, properties = 0, 0, 0, []
    unverified_dictionaries = 0
    unverified_arrays = 0
    with olefile.OleFileIO(io.BytesIO(data)) as oracle:
        streams = {'/'+'/'.join(p): oracle.get_size(p) for p in oracle.listdir()}
        assert streams == {s['path']: s['bytes'] for s in doc.summary['streams']}
        assert oracle.root.clsid.lower() == metadata.identification.root_clsid
        expected = {}
        for name in streams:
            if not name.split('/')[-1].startswith('\x05'):
                continue
            raw = oracle.openstream(name.lstrip('/')).read()
            assert raw[:2] == b'\xfe\xff'
            section_count = struct.unpack_from('<I', raw, 24)[0]
            assert section_count in (1, 2)
            for index in range(section_count):
                fmtid = str(uuid.UUID(bytes_le=raw[28+20*index:44+20*index]))
                offset = struct.unpack_from('<I', raw, 44+20*index)[0]
                count = struct.unpack_from('<I', raw, offset+4)[0]
                offsets = {pid: offset+relative for pid,relative in (struct.unpack_from('<II', raw, offset+8+i*8) for i in range(count))}
                pids = set(offsets)
                if index == 0:
                    values = oracle.getproperties(name.lstrip('/'))
                else:
                    # olefile reads only the first section. An in-memory CFB
                    # view promotes the requested section header, without
                    # changing any property table, value, offset or input file.
                    view_bytes = bytearray(raw)
                    struct.pack_into('<I', view_bytes, 24, 1)
                    view_bytes[28:48] = raw[28+20*index:48+20*index]
                    with olefile.OleFileIO(io.BytesIO(data), write_mode=True) as view:
                        view.write_stream(name.lstrip('/'), bytes(view_bytes))
                        values = view.getproperties(name.lstrip('/'))
                expected[(name, fmtid)] = (pids, values, raw, offsets)
        assert set(expected) == {(s.stream, s.fmtid) for s in metadata.property_sets}
        for s in metadata.property_sets:
            pids, values, raw, offsets = expected[(s.stream, s.fmtid)]
            assert {p.pid for p in s.properties} == pids
            assert set(values) == pids, (item['file'], s.stream, 'oracle omitted PIDs')
            for p in s.properties:
                assert p.status == 'decoded', (item['file'], p.pid, p.status)
                assert p.state_binding == 'unresolved'
                assert p.raw_data == raw[p.source.start_offset:p.source.end_offset]
                other = values[p.pid]
                vt = p.type_code
                if vt is None or vt & 0x2000:
                    # olefile has no dictionary-value oracle. Preserve this gap;
                    # do not turn equality of raw bytes into a semantic check.
                    if vt is None:
                        unverified_dictionaries += 1
                    else:
                        unverified_arrays += 1
                    properties.append({'fmtid':s.fmtid, 'pid':p.pid, 'type_code':vt,
                                       'comparison':'dictionary_not_supported_by_olefile' if vt is None else 'array_not_supported_by_olefile',
                                       'raw_sha256':hashlib.sha256(p.raw_data).hexdigest()})
                    continue
                if vt == 31:
                    other = other.removesuffix('\0')  # olefile retains LPWSTR terminator
                elif vt == 72:
                    other = other.lower()
                elif vt == 2 and other > 32767:
                    other -= 65536  # OLEPS VT_I2 is signed, olefile exposes unsigned
                actual = p.value
                comparison = 'olefile_value'
                if vt in (4, 5):
                    # olefile 0.46 does not decode floats. This independent
                    # wire check uses Python struct and the oracle's PID table.
                    other = struct.unpack_from('<f' if vt == 4 else '<d', raw, offsets[p.pid]+4)[0]
                    comparison = 'python_struct_float_wire_value'
                elif vt == 64:
                    assert actual.ticks_100ns // 10_000_000 == other
                    # olefile loses fractional seconds. Compare raw wire ticks
                    # separately; do not call its timestamp a 100 ns oracle.
                    assert actual.ticks_100ns == struct.unpack_from('<Q', raw, offsets[p.pid]+4)[0]
                    actual = actual.ticks_100ns // 10_000_000
                    times += 1
                    comparison = 'olefile_whole_seconds_and_separate_wire_ticks'
                elif vt == 71:
                    actual = struct.pack('<I', actual.format) + actual.data
                assert actual == other, (item['file'], s.fmtid, p.pid, vt, repr(actual)[:80], repr(other)[:80])
                matched += 1
                if isinstance(actual, bytes):
                    value = {'sha256': hashlib.sha256(actual).hexdigest(), 'bytes': len(actual)}
                    binary += 1
                else:
                    value = actual
                properties.append({'fmtid':s.fmtid, 'pid':p.pid, 'type_code':vt, 'comparison':comparison,
                                   'value':value, 'raw_sha256':hashlib.sha256(p.raw_data).hexdigest()})
        thumbnails = []
        for thumbnail in metadata.thumbnails:
            raw = oracle.openstream(thumbnail.source.stream.lstrip('/')).read()
            assert thumbnail.data == raw[thumbnail.source.start_offset:thumbnail.source.end_offset]
            with Image.open(io.BytesIO(thumbnail.data)) as image:
                assert image.format == 'PNG' and image.size == (thumbnail.width, thumbnail.height)
                image.load()  # Independent decompression/pixel decoding, not only a header check.
            thumbnails.append({'sha256':hashlib.sha256(thumbnail.data).hexdigest(), 'bytes':len(thumbnail.data),
                               'width':thumbnail.width,'height':thumbnail.height,'source':asdict(thumbnail.source)})
    return {'file':item['file'], 'sha256':item['sha256'], 'split':item['split'], 'kind':metadata.identification.kind,
            'property_sets':len(metadata.property_sets), 'matched_properties':matched, 'binary_properties':binary,
            'filetime_properties':times, 'unverified_dictionaries':unverified_dictionaries, 'unverified_arrays':unverified_arrays, 'properties':properties, 'thumbnails':thumbnails,
            'diagnostics':[asdict(d) for d in metadata.diagnostics], 'state_verification':'unverified'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=ROOT/'fixtures/public')
    parser.add_argument('--split', choices=['all','regression','holdout'], default='all')
    parser.add_argument('--output', type=Path, default=ROOT/'internal/reports/latest/document_validation.json')
    args = parser.parse_args()
    if olefile.__version__ != '0.46':
        raise RuntimeError('This oracle is qualified with olefile 0.46; 0.47 has an LPWSTR length-offset regression')
    results = []
    for item in load_manifest(split=args.split):
        result = compare(item,args.fixtures)
        results.append(result)
        print(result['file'],result['kind'],result['matched_properties'],'properties,',len(result['thumbnails']),'PNG',flush=True)
    report = {'scope':'Independent OLE property values and PNG decoding; no vendor state/property inheritance oracle.',
              'providers':{'olefile':olefile.__version__,'Pillow':Image.__version__},
              'comparison_counts':dict(Counter(p['comparison'] for r in results for p in r['properties'])),
              'limitations':['olefile 0.47 reads LPWSTR length at the wrong offset; this oracle pins 0.46.',
                             'Second property sections are selected in temporary in-memory views; their property bytes and offsets remain unchanged.',
                             'olefile 0.46 does not interpret floats, arrays or dictionaries. Float wire values use Python struct; array/dictionary semantics have controlled tests only.',
                             'FILETIME whole seconds are checked with olefile; fractional ticks are a separate wire check.',
                             'These are saved property observations, not effective Inventor Model State values.'],
              'by_split':{split:{'files':sum(r['split']==split for r in results),
                                 'matched_properties':sum(r['matched_properties'] for r in results if r['split']==split),
                                 'unverified_dictionaries':sum(r['unverified_dictionaries'] for r in results if r['split']==split),
                                 'unverified_arrays':sum(r['unverified_arrays'] for r in results if r['split']==split),
                                 'thumbnails':sum(len(r['thumbnails']) for r in results if r['split']==split)} for split in ['regression','holdout']},
              'kinds':dict(Counter(r['kind'] for r in results)), 'results':results}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2,allow_nan=False)+'\n')


if __name__ == '__main__':
    main()
