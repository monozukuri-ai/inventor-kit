#!/usr/bin/env python3
"""Independent four-byte SAB framing oracle; accepts one file and active count."""
from pathlib import Path
import json
import sys
from ezdxf.acis.sab import Decoder

decoder = Decoder(Path(sys.argv[1]).read_bytes())
header = decoder.read_header()
records = []
for _ in range(int(sys.argv[2])):
    start = decoder.index
    tokens = decoder.read_record()
    records.append([tokens[0].value, tokens[1].value, tokens[2].value, start, decoder.index])
print(json.dumps({'save_version': header.version, 'records': records}))
