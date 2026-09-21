"""Check regression IDW framing, provisional fields and source spans; no semantic oracle."""
import argparse
from collections import Counter
import hashlib
import json
import math
from pathlib import Path
import subprocess
import struct
import uuid
import zlib

from corpus_manifest import ROOT
from drawing_corpus import drawing_rows

BASELINE = ROOT / 'tests/data/drawing-inventory-baseline.json'
MAX_EXPANDED = 128 * 1024 * 1024


def snapshot(doc):
    return dict(file=Path(doc['source_id']).name, sha256=doc['source_sha256'], status=doc['status'],
        segments=[dict(kind=s['registry']['kind'], major=s['registry']['major'], status=s['status'],
                       framed_records=len(s['records']), diagnostics=[d['code'] for d in s['diagnostics']],
                       observations=dict(sorted(Counter(o['proposed_role'] for o in s['observations']).items()))) for s in doc['segments']],
        fields_sha256=hashlib.sha256(json.dumps([
            [s['registry']['id'], o['record_ordinal'], o['type_id'], o['proposed_role'],
             [[f['name'], f['value']] for f in o['fields']]]
            for s in doc['segments'] for o in s['observations']
        ], sort_keys=True, ensure_ascii=False).encode()).hexdigest(),
        reference_tables_sha256=hashlib.sha256(json.dumps([
            [s['registry']['id'], t['section'], t['count'], t['bytes']]
            for s in doc['segments'] if s['meta'] for t in s['meta']['reference_tables']
        ], sort_keys=True).encode()).hexdigest(),
        types=dict(sorted(Counter(r['type_id'] for s in doc['segments'] for r in s['records']).items())))


def verify_sources(doc, source):
    import olefile
    import zstandard
    if doc['drawing_semantics'] != 'not_decoded' or doc['sheet_count'] is not None:
        raise ValueError('Structural inventory must not claim drawing semantics/sheet count')
    if doc['source_sha256'] != hashlib.sha256(source.read_bytes()).hexdigest():
        raise ValueError('Drawing report source identity mismatch')
    expanded, stored = {}, {}
    with olefile.OleFileIO(source) as cfb:
        def raw(path):
            if path not in stored:
                stored[path] = cfb.openstream(path.lstrip('/')).read()
            return stored[path]
        for segment in doc['segments']:
            for key in ('meta', 'bulk'):
                entry = segment[key]
                if entry is None:
                    continue
                src = entry['compressed_source']
                if src['byte_domain'] != 'cfb_stream' or src['source_id'] != doc['source_id']:
                    raise ValueError('Compressed source is not a CFB stream range')
                stored_bytes = raw(src['stream'])
                a, b = src['start_offset'], src['end_offset']
                if not 0 <= a <= b <= len(stored_bytes) or src['stream'] in expanded:
                    raise ValueError('Invalid or repeated compressed source')
                encoded = stored_bytes[a:b]
                size = entry['expanded_bytes']
                if not 0 <= size <= MAX_EXPANDED - sum(map(len, expanded.values())):
                    raise ValueError('Expanded source budget exceeded')
                if entry['codec'] == 'zstd':
                    decoded = zstandard.ZstdDecompressor(max_window_size=64*1024*1024).decompress(
                        encoded, max_output_size=size+1, allow_extra_data=False)
                elif entry['codec'] == 'zlib':
                    decoder = zlib.decompressobj()
                    decoded = decoder.decompress(encoded, size+1)
                    if not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
                        raise ValueError('Incomplete/extra compressed member')
                else:
                    raise ValueError('Unknown codec')
                if len(decoded) != size:
                    raise ValueError('Expanded byte count mismatch')
                expanded[src['stream']] = decoded

        def take(span):
            if span['source_id'] != doc['source_id']:
                raise ValueError('Span source identifier mismatch')
            if span['byte_domain'] == 'cfb_stream':
                data = raw(span['stream'])
            elif span['byte_domain'] == 'inflated_stream':
                data = expanded[span['stream']]
            else:
                raise ValueError('Unknown byte domain')
            a, b = span['start_offset'], span['end_offset']
            if not 0 <= a <= b <= len(data):
                raise ValueError('Source span escapes its byte domain')
            return data[a:b]

        revisions = doc.get('revisions')
        if revisions:
            data = take(revisions['source'])
            if struct.unpack_from('<II', data) != (3, len(revisions['entries'])):
                raise ValueError('Revision table header mismatch')
            offset, ids = 8, set()
            for entry in revisions['entries']:
                value = take(entry['source'])
                identity = str(uuid.UUID(bytes_le=value[:16]))
                flags, kind = struct.unpack_from('<IH', value, 16)
                length = 22
                if kind == 65535:
                    if value[22] not in (0, 1):
                        raise ValueError('Unknown revision payload')
                    length += 1 + (16 if value[22] == 0 else 8)
                if (identity in ids or identity != entry['id'] or flags != entry['flags']
                    or kind != entry['kind'] or len(value) != length
                    or value != data[offset:offset+length]
                    or entry['source']['stream'] != revisions['source']['stream']
                    or entry['source']['start_offset'] != revisions['source']['start_offset'] + offset):
                    raise ValueError('Revision identity source mismatch')
                ids.add(identity)
                offset += length
            if offset != len(data):
                raise ValueError('Revision source has trailing data')

        count = 0
        for segment in doc['segments']:
            take(segment['registry']['source'])
            meta = segment['meta']
            if meta:
                block_bytes = take(meta['block_table_source'])
                blocks = [int.from_bytes(block_bytes[i:i+4], 'little') for i in range(0,len(block_bytes),4)]
                if blocks != meta['block_words']:
                    raise ValueError('Meta block table source mismatch')
                for table in meta['reference_tables']:
                    size = {2: 10, 7: 32, 8: 20, 10: 8}[table['section']]
                    if (bytes(table['bytes']) != take(table['source'])
                        or len(table['bytes']) != size * table['count']):
                        raise ValueError('Meta reference table source mismatch')
                for t in meta['types']:
                    if str(uuid.UUID(bytes_le=take(t['source']))) != t['type_id']:
                        raise ValueError('Full type GUID source mismatch')
            if segment['status'] != 'framed':
                if segment['records']:
                    raise ValueError('Failed segment retains accepted records')
            else:
                if take(segment['bulk']['terminal_source']) != b'\xff'*4:
                    raise ValueError('Missing bulk terminal marker')
                ordinals = [i for i, value in enumerate(meta['block_words']) if value & 0x80000000]
                if [r['ordinal'] for r in segment['records']] != ordinals:
                    raise ValueError('Active block ordinals mismatch')
                for record in segment['records']:
                    selector = int.from_bytes(take(record['selector_source']), 'little')
                    if selector != record['selector_word'] or selector & 255 != record['type_index']:
                        raise ValueError('Record selector source mismatch')
                    if record['type_id'] != meta['types'][record['type_index']]['type_id']:
                        raise ValueError('Record type table mismatch')
                    block = int.from_bytes(take(record['meta_block_source']), 'little')
                    if block != meta['block_words'][record['ordinal']] or (block & 0x7fffffff) != len(take(record['source'])):
                        raise ValueError('Record payload length/source mismatch')
                    trailer = take(record['trailer_source'])
                    if int.from_bytes(trailer[:4], 'little') not in (0, block & 0x7fffffff):
                        raise ValueError('Record trailer length mismatch')
                    for ref in record['references']:
                        value = take(ref['source'])
                        n = int.from_bytes(value[:4], 'little')
                        if len(value) != n+8 or value[4:4+n].decode() != ref['raw_name'] or int.from_bytes(value[-4:], 'little') != ref['raw_value']:
                            raise ValueError('Named reference source mismatch')
                        if ref['status'] != 'unresolved_not_followed':
                            raise ValueError('Framing must not resolve drawing references')
                    count += 1
            records = {r['ordinal']: r for r in segment['records']}
            seen = set()
            for observation in segment.get('observations', []):
                ordinal = observation['record_ordinal']
                record = records.get(ordinal)
                if (ordinal in seen or record is None or observation['source'] != record['source']
                    or observation['type_id'] != record['type_id'] or observation['status'] != 'unqualified'):
                    raise ValueError('Invalid typed observation owner/status')
                seen.add(ordinal)
                for field in observation['fields']:
                    src, owner = field['source'], record['source']
                    if (any(src[k] != owner[k] for k in ('source_id','stream','byte_domain'))
                        or not owner['start_offset'] <= src['start_offset'] <= src['end_offset'] <= owner['end_offset']):
                        raise ValueError('Typed field escapes its record')
                    verify_field(field['value'], take(src))
            for region in segment['opaque_regions']:
                take(region['source'])
        for region in doc['unclaimed_streams']:
            take(region['source'])
    return count


def verify_field(value, data):
    """Independent byte comparison; does not validate proposed field meanings."""
    encoding, expected = value['encoding'], value['raw']
    if encoding == 'utf16le_counted':
        n = int.from_bytes(data[:4], 'little')
        if len(data) != 4 + n*2 or data[4:].decode('utf-16le') != expected:
            raise ValueError('UTF-16 field source mismatch')
    elif encoding == 'u8':
        if data != bytes([expected]):
            raise ValueError('Byte field source mismatch')
    elif encoding in ('f32le', 'f64le', 'u32le', 'u16le'):
        code = {'f32le':'f', 'f64le':'d', 'u32le':'I', 'u16le':'H'}[encoding]
        values = [expected] if encoding == 'u16le' else expected
        if encoding.startswith('f') and not all(math.isfinite(v) for v in values):
            raise ValueError('Non-finite field')
        # f32 JSON decimal representations are checked by round-tripping bits.
        if struct.pack('<'+code*len(values), *values) != data:
            raise ValueError('Numeric field source mismatch')
    elif encoding == 'compact_transform':
        p = 4 if expected['prefixed'] else 0
        if p and data[:4] != b'\x03\x02\0\0':
            raise ValueError('Compact transform prefix mismatch')
        set_bits, zero = struct.unpack_from('<HH', data, p); p += 4
        actual = []
        for i in range(16):
            a, b = (set_bits>>i)&1, (zero>>i)&1
            if a or b:
                actual.append(-1.0 if a and b else 1.0 if a else 0.0)
            else:
                actual.append(struct.unpack_from('<d', data, p)[0]); p += 8
        if (p != len(data) or set_bits != expected['set'] or zero != expected['zero']
            or actual != expected['values'] or not all(map(math.isfinite, actual))):
            raise ValueError('Compact transform source mismatch')
    else:
        raise ValueError('Unknown typed field encoding')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'internal/reports/latest/drawing_inventory.json')
    args = parser.parse_args()
    rows = [(entry,row) for entry,row in drawing_rows() if row['split'] == 'regression']
    for entry,row in rows:
        path = ROOT / 'fixtures/public' / row['file']
        if path.stat().st_size != row['bytes'] or hashlib.sha256(path.read_bytes()).hexdigest() != row['sha256']:
            raise ValueError('Regression input identity mismatch')
    subprocess.run(['cargo','build','--locked','-p','inventor-core','--example','inspect_drawing'],cwd=ROOT,check=True)
    reader = ROOT / 'target/debug/examples/inspect_drawing'
    if not reader.exists():
        reader = reader.with_suffix('.exe')
    result = subprocess.run([str(reader),*[str(ROOT/'fixtures/public'/r['file']) for _,r in rows]],
                            cwd=ROOT,capture_output=True,text=True,encoding='utf-8',check=True)
    docs = [json.loads(line) for line in result.stdout.splitlines()]
    if len(docs) != len(rows):
        raise ValueError('Incomplete drawing inventory output')
    baseline = json.loads(BASELINE.read_text())
    actual = [snapshot(d) for d in docs]
    if actual != baseline['results']:
        raise ValueError('Drawing structural regression snapshot changed; review framing differences')
    checked = [verify_sources(d, ROOT/'fixtures/public'/r['file']) for d,(_,r) in zip(docs,rows)]
    report = dict(status='passed', scope='framing_and_source_spans_only', holdouts_used=False,
                  semantic_oracle='not_collected', framed_records=sum(checked),
                  unqualified_observations=sum(len(s['observations']) for d in docs for s in d['segments']),
                  results=actual, inventories=docs)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    print(json.dumps({k:v for k,v in report.items() if k not in ('results','inventories')}))


if __name__ == '__main__':
    main()
