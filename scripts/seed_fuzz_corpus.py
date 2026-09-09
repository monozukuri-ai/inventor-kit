"""Seed fuzzers from verified regression inputs only; never from the holdout set."""
import argparse
import hashlib
from pathlib import Path
import zlib

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
