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
curves on the same spline definition have an additive native view. The original model
resolution and finite UV bounds remain unchanged.

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

## Unreleased additions

Matching cq-acis development sources add degree-1/3 saved UV splines with
independent curve and support-normal senses. The additive
`NativeModel.spline_surface_pcurve()` view retains saved-direction knots and
poles, intervals, raw records and support provenance. The 0.3.3 linear view
and shared model API 2 remain unchanged.

For qualified null-inline tolerant coedges/edges with same-support saved UV,
the converter may use the saved edge scalar in millimetres plus one original
model resolution as the 2D/3D deviation bound. This is an observed interpretation
of ASM 22700 / embedded 22601, without vendor confirmation of the field layout.
`source_edge_tolerances` records each source edge, scalar, placement scale,
applicable bound and measured deviation. Source vertex coordinates, 3D/UV
poles, model resolution and finite UV bounds remain unchanged. Saved fit
tolerances and unknown vertex fields supply no additional allowance.

Local validation on 2026-09-13 increases valid individual FTC07 faces from
**192 / 258 to 245 / 258**, and valid finite-UV faces from **0 / 8 to 5 / 8**.
The 195 converted tolerant coedges and eight decoded finite surfaces remain
unchanged. There are 153 legacy linear views and 224 new spline views
(including linear curves). All 33 files retain ten valid-solid conversions
(nine regression and one holdout), with the frozen regression solid metrics
preserved.

The remaining 13 faces are ten unsupported inline coedge cases, two pcurve
mismatches exceeding even the source edge bound, and one unsupported curve.
Complete FTC07 conversion still rejects with `geometry.pcurve_mismatch`.
These results do not qualify current Model State, Inventor equivalence,
Windows/macOS, remote CI or a published distribution.

Install matching cq-acis sources to exercise the additions. The scripts above
also retain the original checks with public 0.3.3; when the new view is present,
they additionally check counts, source provenance and local bounds. Reports
record package versions and loaded module paths to distinguish development
builds from published distributions even before the next version bump.
