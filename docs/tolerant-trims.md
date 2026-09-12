# Tolerant boundaries and finite UV development

English | [日本語](tolerant-trims.ja.md)

This branch follows the matching `cq-acis` development implementation. The
published `acis-core`, `acis-py-bridge` and `cq-acis` 0.3.2 packages do not contain
these additions. The package version and production dependency pins have not
been released or advanced. Normal registry-only CI qualification must follow
publication of the shared dependencies and an explicit dependency update.

The shared Rust parser now retains finite domains of the observed forward
explicit ASM 22700 splines, including subtype references. The converter can use
qualified tolerant edges/coedges and bounded face trims. Saved degree-1 UV
curves on the same spline definition have an additive native view. Local
tolerant fields never enlarge the original model tolerance.

Reproduce the source checks with matching development dependencies installed:

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
