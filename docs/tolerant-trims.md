# Tolerant boundaries and finite UV

English | [日本語](tolerant-trims.ja.md)

Conversion checks saved tolerant edge, vertex and UV bounds before constructing
faces. Supported saved UV fits may be reparameterized while retaining their
original locus and traversal. Original vertex positions and 3D curves remain
fixed. Vertex tolerances do not enlarge curve, UV or edge allowances; the global
model resolution remains 0.00001 mm.

Unsupported or ambiguous tolerance layouts reject conversion. Valid individual
faces alone do not qualify an entire part: `Document.to_cadquery()` must produce
a valid solid while retaining all required source faces. See
[supported scope](support.md) and [body selection](body-conversion.md).

## Reproducing the checks

Rust uses published `acis-core=0.3.8` and `acis-py-bridge=0.3.8`.
Python requires `cq-acis>=0.3.8,<0.4` and shared model API 2.

```sh
python scripts/validate_components.py
python scripts/validate_topology.py
python scripts/validate_tolerant_trims.py
python scripts/validate_geometry.py
```

The scripts check fixture hashes, preserve independent holdouts and record
source bounds and measured errors under `internal/reports/latest/`.
See the [validation guide](validation.md) for comparison methods and limits.
