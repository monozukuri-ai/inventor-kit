# Tolerant boundaries and finite UV

English | [日本語](tolerant-trims.ja.md)

These additions require `acis-core` / `acis-py-bridge` 0.3.3 and
`cq-acis>=0.3.3,<0.4`. The Rust dependencies resolve to crates.io, including the
independent fuzz lockfile. The Python minimum also advances because the converter
and saved pcurve views require the matching Python implementation. Shared model
API 2 is unchanged.

The shared Rust parser now retains finite domains of the observed forward
explicit ASM 22700 splines, including subtype references. The converter can use
qualified tolerant edges/coedges and bounded face trims. Saved degree-1 UV
curves on the same spline definition have an additive native view. Local
tolerant fields never enlarge the original model tolerance.

Reproduce the source checks with the matching dependencies installed:

```sh
python scripts/validate_components.py
python scripts/validate_topology.py
python scripts/validate_tolerant_trims.py
python scripts/validate_geometry.py
```

Each script verifies pinned fixture hashes; detailed reports are written under
`internal/reports/latest/`. The new check uses only the FTC07 2021 regression
file and keeps source spans for its finite surfaces, boundaries and pcurves.
The full geometry check separately retains the holdout split and the frozen
solid metrics. Missing inputs and failed assertions are errors.

In the local development run, FTC07 has 195 converted tolerant coedges, eight
decoded finite surfaces, and 153 saved linear pcurve views. Individually valid
faces increase from 112 to 192 out of 258. Complete FTC07 conversion still
fails at curve-on-surface consistency, and its eight finite faces are not yet
fully convertible. The full corpus remains at ten valid-solid conversions.

Remaining work includes the semantics of local edge/coedge tolerances, inline
curves, additional saved pcurve charts and profiles, and vendor comparison.
These results do not qualify current Model State, native appearance, remote
CI or a published distribution.
