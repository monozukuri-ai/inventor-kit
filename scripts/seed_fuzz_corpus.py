"""Seed fuzzers from verified regression inputs only; never from the holdout set."""
import argparse
import hashlib
from pathlib import Path
import zlib
import struct

from drawing_corpus import drawing_rows

from check_corpus import ROOT, verify


def layered_pair(record):
    """An independently authored Meta7/B pair: one record and its two markers."""
    line_type = bytes.fromhex('c7ac9ea7d11181c26000a38ab46bceb0')
    blocks = (0x80000000 | len(record), 30, 6)
    body = bytes(14) + struct.pack('<I', len(blocks)) + struct.pack(f'<{len(blocks)}I', *blocks)
    body += struct.pack('<I', 4 + 4 * len(blocks)) + struct.pack('<4I', 0, 4, 0, 4)
    body += struct.pack('<I', 1) + line_type + bytes(12) + struct.pack('<I', 32)
    for n in range(5, 11):
        body += struct.pack('<2I', 0xff00ff00 if n == 5 else 0, 4)
    body += struct.pack('<I', 18) + bytes(88)
    version = b'RSe Meta Stream Version 7'
    name = 'DLSheet1DLSegment'.encode('utf-16le')
    meta = struct.pack('<I', len(version)) + version + struct.pack('<H', 7) + bytes(16)
    meta += struct.pack('<I', len(name) // 2) + name + bytes(16) + bytes(12)
    meta += struct.pack('<I', 0) + struct.pack('<I', 0) + b'\x01' + zlib.compress(body)
    records = struct.pack('<I', 0x100) + record + struct.pack('<I', len(record))
    records += struct.pack('<I', 0xffffffff)
    bulk = bytes.fromhex('9ec22ba4d411e80160002db3ee29fbb00401') + zlib.compress(records)
    return struct.pack('<I', len(meta)) + meta + bulk


def seed(fixtures, output):
    inventory = verify(fixtures)
    allowed = {r['file'] for r in inventory['files'] if r['split'] == 'regression'}
    import olefile
    import inventor_kit as ik
    def write(target, data):
        path = output / target / hashlib.sha256(data).hexdigest()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    for name in ('SamplePart.ipt', 'Cylinder.ipt', 'm5-samplebg/Subassembly.iam'):
        if name not in allowed:
            raise ValueError('Fuzz seed is not a regression file: ' + name)
        path = fixtures / name
        write('container', path.read_bytes())
        with olefile.OleFileIO(path) as doc:
            for parts in doc.listdir():
                stream = '/'.join(parts)
                data = doc.openstream(parts).read()
                if stream == 'UFRxDoc':
                    write('streams', b'\x03' + data)
                elif stream == 'RSeStorage/RSeSegInfo':
                    write('streams', b'\x02' + data)
                elif stream.startswith('RSeStorage/M'):
                    write('streams', b'\x01' + data)
                elif stream.startswith('RSeStorage/B'):
                    write('streams', b'\x00' + data[18:])
        if name.endswith('.ipt'):
            kernel = ik.read_file(path).kernel_bytes
            if kernel:
                write('streams', b'\x04' + kernel)
    # Native drawing seeds are regression-only. Large inputs stay outside the
    # fuzz harness file cap; no holdout record layouts are inspected here.
    for _, row in drawing_rows():
        if row['split'] != 'regression':
            continue
        name = row['file']
        if name not in allowed:
            raise ValueError('Drawing fuzz seed is not a regression file: ' + name)
        path = fixtures / name
        if row['bytes'] <= 2 * 1024 * 1024:
            write('drawing', b'\x00' + path.read_bytes())
        with olefile.OleFileIO(path) as doc:
            paths = {tuple(p) for p in doc.listdir()}
            for parts in sorted(paths):
                if len(parts) != 2 or parts[0] != 'RSeStorage' or not parts[1].startswith('M'):
                    continue
                bulk = (parts[0], 'B' + parts[1][1:])
                if bulk not in paths:
                    continue
                meta, encoded = doc.openstream(parts).read(), doc.openstream(bulk).read()
                payload = struct.pack('<I', len(meta)) + meta + encoded
                if len(payload) <= 256 * 1024:
                    write('drawing', b'\x01' + payload)
    # Independently authored framing seed, not an IDW or drawing-value oracle.
    raw = struct.pack('<III', 1, 1, 0x80000001) + bytes(range(16))
    raw += struct.pack('<I', 0) + b'x' + struct.pack('<I', 1) + b'\x00' + struct.pack('<I', 0xffffffff)
    write('drawing', b'\x02' + raw)
    # Synthetic raw typed-field seeds keep fuzzing independent of native oracles.
    text = 'Text 図'.encode('utf-16le')
    write('drawing', b'\x03'+bytes(26)+struct.pack('<I',len(text)//2)+text+
          struct.pack('<6dHIB',1.,2.,0.,1.,0.,0.,9,2,0))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<4I6fB',0x30000002,2,2,0x102,0.,0.,0.,1.,2.,0.,0))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<6IBIHH2d',0x30000002,2,2,0x10,0x80000001,0x80000001,1,0x203,0x8421,0x7b56,1.,2.))
    for tag in (0x30000002, 0x30000003):
        refs = struct.pack('<2I', tag, 1) + (struct.pack('<I', 1) if tag == 0x30000002 else b'')
        # Major28 combines the compact tag with modern flags; vary them independently.
        for flags in (0, 0x10):
            write('drawing', b'\x03'+bytes(26)+refs+struct.pack('<2IB', flags, 0x80000002, 0))
    # Synthetic Meta6/7 (layered) line record and a Meta7/B pair framing it.
    line = bytes(6) + struct.pack('<I', 6) + bytes(20) + struct.pack('<I', 30)
    line += struct.pack('<6d', 0., 0., 0., 1., 1., 0.)
    write('drawing', b'\x03' + line)
    write('drawing', b'\x01' + layered_pair(line))
    write('drawing', b'\x03'+bytes(6)+b'\x01\x01'+struct.pack('<2I', 2, 1)+bytes(5)
          +bytes([11, 22, 33, 255, 44, 55, 66, 0, 1]))
    write('drawing', b'\x03'+bytes(15)+struct.pack('<2I',0x30000002,0)+struct.pack('<IBI',1,1,2)+
          struct.pack('<IHHBI4d',0x203,0x8421,0x7bde,1,2,-1.,-2.,42.,29.7))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<6d',1.,2.,3.,4.,5.,6.))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<7dB',1.,2.,3.,0.,0.,1.,2.,0))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<12dB',1.,2.,3.,0.,0.,1.,1.,0.,0.,2.,0.,1.,0))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<13dB',1.,2.,3.,4.,2.,1.,0.,0.,0.,1.,0.,0.,1.,0))
    # Rational quadratic quarter-circle with exact observed major23 framing.
    spline = bytes(26)+struct.pack('<IIId',1,0,2,1e-9)
    for values, dimension in [([0.,0.,0.,1.,1.,1.],1), ([1.,2**-.5,1.],1),
                               ([1.,0.,0.,1.,1.,0.,0.,1.,0.],3)]:
        count=len(values)//dimension
        spline+=struct.pack('<III',count,count,8)+struct.pack('<'+'d'*len(values),*values)
    write('drawing', b'\x03'+spline+struct.pack('<dIIdd',1e-12,1,1,0.,1.))
    wide = struct.pack('<I',1)+b'A\0'
    write('drawing', b'\x03'+bytes(6)+struct.pack('<4II3H2f',0x30000002,1,1,0x102,77,4,400,0,.5,0.)+wide+struct.pack('<3fI',0.,1.,0.,78))
    write('drawing', b'\x03'+bytes(34)+wide+bytes(134))
    write('drawing', b'\x03'+bytes(34)+wide+struct.pack('<2I',0x30000002,0)+bytes(36)+
          struct.pack('<5I',0x30000002,1,1,0x10,0x80000001))
    write('drawing', b'\x03'+bytes(34)+struct.pack('<2I',0x30000002,0)+bytes(11)+
          wide*3+bytes(180)+wide+bytes(8))
    write('drawing', b'\x03'+struct.pack('<5I',0x30000002,1,1,0x10000000,77))
    write('drawing', b'\x03'+struct.pack('<IB',4,0))
    write('drawing', b'\x03'+struct.pack('<III f B',1,5,0xffffffff,1.,0))
    write('drawing', b'\x03'+struct.pack('<IHfHHBHH2d3fII',8,0,.025,1,0,0,2,2,.4,-.1,-10000.,1.,0.,0,1))
    write('drawing', b'\x03'+bytes(15)+struct.pack('<2dIHHIBI',2.,4.,11,0x8421,0x7bde,0xffffff,2,99))
    # Synthetic baseline JPEG envelope, deliberately not an image-value oracle.
    write('drawing', b'\x03'+bytes([255,216,255,192,0,11,8,0,2,0,3,1,1,17,0,255,218,0,8,1,1,0,0,63,0,7,255,217]))
    for selector in range(4):
        write('drawing', bytes([selector]) + b'\xff' * 32)
    # Cheap expanding stream plus integer-length extremes, independently authored.
    write('streams', b'\x00' + zlib.compress(bytes(1024 * 1024)))
    for selector in range(5):
        write('streams', bytes([selector]) + b'\xff' * 32)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'fixtures/public')
    parser.add_argument('--output', type=Path, default=ROOT / 'fuzz/corpus')
    args = parser.parse_args()
    seed(args.fixtures, args.output)


if __name__ == '__main__':
    main()
