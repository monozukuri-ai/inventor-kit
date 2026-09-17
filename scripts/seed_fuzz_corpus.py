"""Seed fuzzers from verified regression inputs only; never from the holdout set."""
import argparse
import hashlib
from pathlib import Path
import zlib
import struct

from drawing_corpus import drawing_rows

from check_corpus import ROOT, verify


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
        if row['split'] != 'regression' or row['bytes'] > 2 * 1024 * 1024:
            continue
        name = row['file']
        if name not in allowed:
            raise ValueError('Drawing fuzz seed is not a regression file: ' + name)
        path = fixtures / name
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
    write('drawing', b'\x03'+bytes(15)+struct.pack('<2I',0x30000002,0)+struct.pack('<IBI',1,1,2)+
          struct.pack('<IHHBI4d',0x203,0x8421,0x7bde,1,2,-1.,-2.,42.,29.7))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<6d',1.,2.,3.,4.,5.,6.))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<7dB',1.,2.,3.,0.,0.,1.,2.,0))
    write('drawing', b'\x03'+bytes(26)+struct.pack('<12dB',1.,2.,3.,0.,0.,1.,1.,0.,0.,2.,0.,1.,0))
    wide = struct.pack('<I',1)+b'A\0'
    write('drawing', b'\x03'+bytes(6)+struct.pack('<4II3H2f',0x30000002,1,1,0x102,77,4,400,0,.5,0.)+wide+struct.pack('<3fI',0.,1.,0.,78))
    write('drawing', b'\x03'+bytes(34)+wide+bytes(134))
    write('drawing', b'\x03'+bytes(34)+wide+struct.pack('<2I',0x30000002,0)+bytes(36)+
          struct.pack('<5I',0x30000002,1,1,0x10,0x80000001))
    write('drawing', b'\x03'+bytes(34)+struct.pack('<2I',0x30000002,0)+bytes(11)+
          wide*3+bytes(180)+wide+bytes(8))
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
