"""Synthetic mutations of regression inputs; never native qualification evidence."""
from io import BytesIO
import struct

import olefile


def with_segment_major(data, major):
    """Retain the CFB and previews while changing every registry version byte."""
    stream = BytesIO(data)
    with olefile.OleFileIO(stream, write_mode=True) as cfb:
        path = 'RSeStorage/RSeSegInfo'
        registry = bytearray(cfb.openstream(path).read())
        position = 0

        def word():
            nonlocal position
            value, = struct.unpack_from('<I', registry, position)
            position += 4
            return value

        def wide():
            nonlocal position
            size = word()
            position += size * 2

        count = word()
        assert 0 < count < 100
        for _ in range(count):
            wide()
            position += 16 + 20
            objects = word()
            position += 24
            wide()
            position += 8
            registry[position + 2] = major
            position += 12
            nodes = 1
            for _ in range(objects):
                position += 45
                nodes = word()
            assert nodes >= 1
            position += (nodes - 1) * 22
        assert position <= len(registry)
        cfb.write_stream(path, bytes(registry))
        return stream.getvalue()
