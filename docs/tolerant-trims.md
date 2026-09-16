# Tolerant boundaries and finite UV

English | [日本語](tolerant-trims.ja.md)

Rust uses public crates.io `acis-core=0.3.8` and `acis-py-bridge=0.3.8`;
the fuzz graph pins the same core. Python requires `cq-acis>=0.3.8,<0.4`, with
public PyPI 0.3.8 in `uv.lock`. Shared model API 2 is unchanged. The trim checks
also pass with the published 0.3.8 wheel. The 0.3.6 results below document the
original trim fix; see [current validation results](validation.md) for the
additional FTC06 and CTC04 closure checks.

## Reproducing the checks

```sh
python scripts/validate_components.py
python scripts/validate_topology.py
python scripts/validate_tolerant_trims.py
python scripts/validate_geometry.py
```

Scripts check fixture hashes, preserve independent holdouts and write reports
under `internal/reports/latest/`. The trim report includes loaded module paths,
source spans and hashes, support ownership, source bounds and measured errors.
It requires the complete reconciliation capability introduced in cq-acis 0.3.6.
A complete-part result requires the actual
`Document.to_cadquery()` entrypoint and a valid solid retaining all 258 faces.

## Four faces resolved in cq-acis 0.3.6

| Faces | Change | Measured / allowed (mm) |
| --- | --- | --- |
| 1164 | Exact saved UV knot subdivision and monotone parameter mapping | 0.006120595657 / 0.006130569677 |
| 2336 | Same method, with the original TEDGE bound | 0.006099236931 / 0.006109190844 |
| 332, 4351 | Source vertex 3618 retained; all three incident source edges checked | 0.006526083452 / 0.006536083452 |
| 4351 | Associated saved UV fit reparameterized, independently of the 3D fit | 0.000748651878 / 0.001049799677 |

Knot insertion preserves the full original UV locus and traversal. Each
resulting span and the complete curve pass independent 3D checks; finite UV
bounds are unchanged. No projected curve outside the chart is adopted.
`saved_pcurve_reparameterizations` records both parameter clocks and errors.

The vertex rule is an observed ASM 22700 / 22601 profile: flag 1, legacy scalar
-1, a checked full intersection-curve owner, and two stored bounds tightly
matching the complete incident-edge endpoint star. Bounds that are too small,
loose, ambiguous, or consume an incident edge reject conversion. The saved
point and 3D curves remain fixed. `tolerant_vertex_envelopes` records the source
values and all incident distances. This does not qualify all TVERTEX layouts.

A saved UV fit attached to the same independently checked intersection support
uses its own fit bound. The unchanged 3D fit must separately remain within its
own bound on both original supports. Vertex envelopes never expand curve/UV
fit or TEDGE allowances. The global model resolution stays at 0.00001 mm.

## Public-dependency results (2026-09-14)

| Check | 0.3.5 sources before this change | Public cq-acis 0.3.6 |
| --- | ---: | ---: |
| Decoded inline coedges | 10 / 10 | 10 / 10 |
| Valid tolerant coedge boundaries | 206 / 206 | 206 / 206 |
| Valid individual FTC07 faces | 254 / 258 | 258 / 258 |
| Valid finite-UV faces | 5 / 8 | 8 / 8 |
| Complete FTC07 solid | rejected | valid, 1 solid / 258 faces |
| Saved spline pcurve views | 224 | 224 |

At the 0.3.6 baseline, the 33-file corpus had eleven valid solid conversions:
ten regressions and one holdout. Existing frozen regression metrics are preserved. FTC07 volume is
1678794.5921294673 mm³. The original 254 valid face areas are unchanged.

These results establish local saved-geometry consistency and OCCT validity.
Inventor/current Model State, Windows/macOS and remote CI remain unverified.
Local wheel and sdist installation checks use the public cq-acis wheel and
registry Rust dependencies, with interpreter shutdown and STEP roundtrip checks.
